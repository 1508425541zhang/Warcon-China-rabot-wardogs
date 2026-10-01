use super::{Context, failure, go, segment};
use crate::error::{ApiError, Result};
use axum::http::Method;
use serde_json::{Value, json};
pub async fn load(c: &mut Context) -> Result<Value> {
    let source = c.input.source.clone();
    let a = c.account().await?;
    let next = crate::identity_crypto::safe_path(&c.query("next"), "/");
    match source.as_str() {
        "src/routes/(auth)/sign-in/+page.server.ts" => {
            if a.is_some() {
                return Ok(go(303, &next));
            }
            let cfg = c.get("/api/identity/configuration").await?;
            if cfg["initialized"] != true {
                return Ok(go(303, "/setup"));
            }
            Ok(json!({"discord":cfg["discord"],"next":next,"orgSignup":cfg["orgSignup"]}))
        }
        "src/routes/(auth)/setup/+page.server.ts" => {
            let cfg = c.get("/api/identity/configuration").await?;
            if cfg["initialized"] == true {
                return Ok(go(303, "/sign-in"));
            }
            Ok(json!({"tokenRequired":cfg["tokenRequired"]}))
        }
        "src/routes/(auth)/recover/+page.server.ts" => Ok(if a.is_some() {
            go(303, "/account")
        } else {
            json!({})
        }),
        "src/routes/(auth)/sign-in/verify/+page.server.ts" => {
            if a.is_some() {
                return Ok(go(303, &next));
            }
            let challenge = c
                .headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';').any(|p| {
                        let p = p.trim();
                        p.strip_prefix("warcon.two_factor=")
                            .or_else(|| p.strip_prefix("__Secure-warcon.two_factor="))
                            .is_some_and(|v| !v.is_empty())
                    })
                });
            Ok(if challenge {
                json!({"next":next})
            } else {
                go(303, "/sign-in")
            })
        }
        "src/routes/(auth)/sign-up/+page.server.ts" => {
            let cfg = c.get("/api/identity/configuration").await?;
            if cfg["orgSignup"] != true {
                return Err(ApiError::missing());
            }
            if cfg["initialized"] != true {
                return Ok(go(303, "/setup"));
            }
            let remaining = match a {
                Some(a) => super::management::remaining(&c.state, &a).await?,
                None => Value::Null,
            };
            Ok(
                json!({"discord":cfg["discord"],"turnstileSiteKey":cfg["turnstileSiteKey"],"remaining":remaining}),
            )
        }
        "src/routes/(auth)/join/[token]/+page.server.ts" => {
            let token = c.param("token").to_owned();
            let inv = match c
                .get(&format!("/api/identity/invites/{}", segment(&token)))
                .await
            {
                Ok(v) => v,
                Err(e) if e.status == axum::http::StatusCode::NOT_FOUND => {
                    return Ok(
                        json!({"valid":false,"problem":"This invite link is not valid.","discord":false}),
                    );
                }
                Err(e) => return Err(e),
            };
            let problem = if inv["org"]["suspended"] == true {
                Some("This organisation is suspended.")
            } else if inv["invite"]["revokedAt"].is_string()
                || inv["invite"]["expiresAt"]
                    .as_str()
                    .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                    .is_some_and(|v| v < chrono::Utc::now())
                || inv["invite"]["maxUses"]
                    .as_i64()
                    .is_some_and(|n| inv["invite"]["uses"].as_i64().unwrap_or(0) >= n)
            {
                Some("This invite link can no longer be used.")
            } else {
                None
            };
            let already =
                match a {
                    Some(a) => sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM org_members WHERE org_id=$1 AND user_id=$2)",
                    )
                    .bind(inv["org"]["id"].as_str().unwrap_or(""))
                    .bind(a.id)
                    .fetch_one(&c.state.db)
                    .await?,
                    None => false,
                };
            let cfg = c.get("/api/identity/configuration").await?;
            Ok(
                json!({"valid":problem.is_none(),"problem":problem,"org":{"id":inv["org"]["id"],"name":inv["org"]["name"]},"orgRole":inv["invite"]["orgRole"],"serverRole":inv["invite"]["serverRoleName"],"alreadyMember":already,"discord":cfg["discord"],"turnstileSiteKey":cfg["turnstileSiteKey"]}),
            )
        }
        "src/routes/(app)/account/+page.server.ts" => {
            if a.is_none() {
                return Ok(go(303, "/sign-in"));
            }
            c.get("/api/identity/account").await
        }
        _ => Err(ApiError::missing()),
    }
}
pub async fn action(c: &mut Context) -> Result<Value> {
    let source = c.input.source.clone();
    let action = c.input.action.clone();
    let next = crate::identity_crypto::safe_path(&c.query("next"), "/");
    let mut body = c.fields();
    let result:Result<Value>=async {
        match source.as_str(){
            "src/routes/(auth)/setup/+page.server.ts" if action=="password"=>{body["name"]=json!(c.text("displayName",80));let v=c.api(Method::POST,"/api/identity/setup",body).await?;Ok(go(303,v["redirect"].as_str().unwrap_or("/")))}
            "src/routes/(auth)/sign-in/+page.server.ts" if action=="password"=>{body["next"]=json!(next);let v=c.api(Method::POST,"/api/identity/login",body).await?;if v["twoFactorRedirect"]==true{Ok(go(303,&format!("/sign-in/verify?next={}",segment(&next))))}else{Ok(go(303,v["redirect"].as_str().unwrap_or(&next)))}}
            "src/routes/(auth)/sign-in/verify/+page.server.ts" if action=="default"=>{body["trustDevice"]=json!(c.raw("trust")=="on");body["next"]=json!(next);let v=c.api(Method::POST,"/api/identity/verify",body).await?;Ok(go(303,v["redirect"].as_str().unwrap_or(&next)))}
            "src/routes/(auth)/recover/+page.server.ts" if action=="default"=>{let v=c.api(Method::POST,"/api/identity/recover",body).await?;Ok(go(303,v["redirect"].as_str().unwrap_or("/account?recovered=1")))}
            "src/routes/(app)/account/+page.server.ts"=>{
                if c.account().await?.is_none(){return Ok(go(303,"/sign-in"));}
                if ["linkSteam","linkDiscord"].contains(&action.as_str()){return provider(c,if action=="linkSteam"{"steam"}else{"discord"},"link","/account?linked=","/account","").await;}
                if !["defaultOrg","steam","password","removePassword","totpStart","totpConfirm","totpDisable","backupCodes","recoveryKey","recoveryKeyClear","revoke","unlinkDiscord","unlinkSteam","deleteAccount"].contains(&action.as_str()){return Err(ApiError::missing());}
                let v=c.api(Method::POST,&format!("/api/identity/account/{action}"),body).await?;
                if action=="deleteAccount"{return Ok(go(303,v["redirect"].as_str().unwrap_or("/sign-in?deleted=1")));}
                if action=="password"&&!c.query("force").is_empty(){return Ok(go(303,"/"));}Ok(v)
            }
            "src/routes/(auth)/sign-in/+page.server.ts" if ["steam","discord"].contains(&action.as_str())=>{let path=if next=="/"&&c.state.config.identity.allow_signup{"/sign-up".to_owned()}else{next};let back=format!("/sign-in?next={}",segment(&path));provider(c,&action,"signin",&path,&back,"").await}
            "src/routes/(auth)/sign-up/+page.server.ts"|"src/routes/(auth)/join/[token]/+page.server.ts"=>{
                let invite=if source.contains("/join/"){c.param("token").to_owned()}else{String::new()};
                if invite.is_empty()&&!c.state.config.identity.allow_signup{return Err(ApiError::missing());}
                let here=if invite.is_empty(){"/sign-up".to_owned()}else{format!("/join/{}",segment(&invite))};
                match action.as_str(){
                    "discord"|"steam"=>provider(c,&action,"signin",&here,&here,&invite).await,
                    "register"=>{if c.account().await?.is_some(){return Ok(go(303,&here));}body["next"]=json!(here);body["invite"]=json!(invite);body["turnstile"]=json!(c.text("cf-turnstile-response",4000));c.api(Method::POST,"/api/identity/register",body).await?;Ok(go(303,&here))}
                    "join" if !invite.is_empty()=>{if c.account().await?.is_none(){return Ok(go(303,&format!("/sign-in?next={}",segment(&here))));}c.api(Method::POST,&format!("/api/identity/invites/{}",segment(&invite)),json!({})).await?;Ok(go(303,"/"))}
                    "create" if invite.is_empty()=>{if c.account().await?.is_none(){return Ok(go(303,"/sign-in?next=%2Fsign-up"));}let v=c.api(Method::POST,"/api/orgs",json!({"name":c.text("orgName",60)})).await?;Ok(go(303,&format!("/orgs/{}",segment(v["id"].as_str().unwrap_or("")))))}
                    _=>Err(ApiError::missing()),
                }
            }
            "src/routes/(app)/qq-link/+page.server.ts"=>{let endpoint=match action.as_str(){"bind"=>"/api/account/qq/bind","unbind"=>"/api/account/qq/unbind",_=>return Err(ApiError::missing())};c.api(Method::POST,endpoint,body).await?;Ok(json!({"message":if action=="bind"{"绑定成功，可以回 QQ 群使用机器人。"}else{"已解绑，Steam 账号的积分保留。"}}))}
            _=>Err(ApiError::missing()),
        }
    }.await;
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            let mut data = if source.contains("/qq-link/") {
                json!({"message":e.message})
            } else {
                json!({"error":e.message})
            };
            if source.contains("/(auth)/") {
                for key in ["username", "displayName", "orgName"] {
                    if c.input.form.get(key).is_some() {
                        data[key] = json!(c.text(key, 80));
                    }
                }
            }
            if action == "totpConfirm" {
                data["totpRetry"] = json!(true);
            }
            Ok(failure(e.status.as_u16(), data))
        }
    }
}
async fn provider(
    c: &mut Context,
    provider: &str,
    mode: &str,
    next: &str,
    back: &str,
    invite: &str,
) -> Result<Value> {
    let next = if mode == "link" {
        format!("{next}{provider}")
    } else {
        next.into()
    };
    let v = c
        .api(
            Method::POST,
            &format!("/api/identity/providers/{provider}"),
            json!({"mode":mode,"next":next,"back":back,"invite":invite}),
        )
        .await?;
    let path = v["url"]
        .as_str()
        .ok_or_else(|| ApiError::bad("Provider did not return an authorization URL."))?;
    Ok(go(303, path))
}

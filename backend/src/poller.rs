//! Single-owner scheduler with stable phases, cadence and a bounded offline share.
use crate::{
    config::AppState,
    dispatcher,
    error::Result,
    observation_state,
    observer::{self, Memory, number},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use tokio::task::JoinSet;
#[derive(Default)]
struct Wake {
    identity: bool,
    lists: bool,
}
fn wake(m: &mut Memory, w: &Wake) {
    let now = observer::now();
    m.players_due = now.max(m.hold);
    m.status_due = now.max(m.hold);
    if w.identity {
        m.identity_at = 0
    }
    if w.lists {
        m.sync_at = 0;
        m.lists_at = 0
    }
}
pub async fn run(app: AppState) -> Result<()> {
    if app.runtime.leader.get().is_none() {
        return Err(crate::runtime::lost());
    }
    let mut listener = sqlx::postgres::PgListener::connect_with(&app.db).await?;
    listener.listen("warcon_observe").await?;
    let mut beat = tokio::time::interval(Duration::from_millis(250));
    beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut settings = crate::settings::load(&app.db).await?;
    let mut memories: HashMap<String, Memory> = HashMap::new();
    let mut in_flight = HashMap::<String, bool>::new();
    let mut flight_started = HashMap::<String, i64>::new();
    let mut last_players = HashMap::<String, usize>::new();
    let mut launched = 0u64;
    let mut present = HashSet::<String>::new();
    let mut signature = HashMap::<String, String>::new();
    let mut wakes = HashMap::<String, Wake>::new();
    let mut tasks: JoinSet<(Memory, Result<()>)> = JoinSet::new();
    let mut roster_at = 0;
    let mut settings_at = 0;
    loop {
        tokio::select! {
         _=app.runtime.stop.cancelled()=>break,
         done=tasks.join_next(),if !tasks.is_empty()=>{
          let Some(Ok((mut m,result)))=done else{app.runtime.stop.cancel();return Err(crate::runtime::lost())};
          in_flight.remove(&m.id);
          flight_started.remove(&m.id);
          last_players.insert(m.id.clone(),if m.ok{m.players.len()}else{0});
          if let Err(e)=result{if e.code=="worker_ownership_lost"{app.runtime.stop.cancel();break}tracing::warn!(server_id=%m.id,code=%e.code,"Observation stage failed");m.sessions=None;}
          let now=observer::now();let (pc,sc)=m.cadence(&settings,m.tier(&app));
          m.players_due=if m.players_due<=now{now+pc}else{m.players_due.min(now+pc)}.max(m.hold);m.status_due=if m.status_due<=now{now+sc}else{m.status_due.min(now+sc)}.max(m.hold);
          if let Some(w)=wakes.remove(&m.id){wake(&mut m,&w)}
          if present.contains(&m.id){memories.insert(m.id.clone(),m);}
         },
         notification=listener.recv()=>{
          if let Ok(note)=notification{if let Ok(event)=serde_json::from_str::<Value>(note.payload()){
           if let Some(ids)=event["interest"].as_array(){let ids=ids.iter().filter_map(Value::as_str).filter(|s|s.len()<=64&&present.contains(*s)).map(str::to_owned).collect::<Vec<_>>();app.runtime.interest(&ids,number(&settings,"watchLeaseMs",15000) as u64);for id in ids{if let Some(m)=memories.get_mut(&id){wake(m,&Wake::default());}}}
           else{
            let ids=if let Some(id)=event["serverId"].as_str(){vec![id.to_owned()]}else if let Some(org)=event["orgId"].as_str(){memories.values().filter(|m|m.org==org).map(|m|m.id.clone()).chain(in_flight.keys().cloned()).collect()}else{present.iter().cloned().collect()};
            for id in ids{let w=Wake{identity:event["identity"]==true,lists:event["lists"]==true||event["orgId"].is_string()};if let Some(m)=memories.get_mut(&id){wake(m,&w)}else if in_flight.contains_key(&id){let pending=wakes.entry(id).or_default();pending.identity|=w.identity;pending.lists|=w.lists;}}
            roster_at=0;
           }
          }}else{roster_at=0;settings_at=0;tokio::time::sleep(Duration::from_millis(250)).await;}
         },
         _=beat.tick()=>{
          if let Err(e)=app.runtime.check().await{if e.code=="worker_ownership_lost"{return Err(e)}tracing::warn!("Worker ownership check unavailable; pausing observations");continue}
          let now=observer::now();
          let mut tiers=json!({"watched":0,"hot":0,"idle":0,"offline":0});
          let mut behind=0;
          for m in memories.values(){let tier=m.tier(&app);tiers[tier]=json!(tiers[tier].as_u64().unwrap_or(0)+1);let(p,s)=m.cadence(&settings,tier);if m.players_due.min(m.status_due)>0&&now-m.players_due.min(m.status_due).max(m.hold)>p.min(s){behind+=1}}
          for(id,offline)in &in_flight{let tier=if *offline{"offline"}else if app.runtime.watched(id){"watched"}else if last_players.get(id).copied().unwrap_or(0)>0{"hot"}else{"idle"};tiers[tier]=json!(tiers[tier].as_u64().unwrap_or(0)+1);}
          *app.runtime.poller.lock().unwrap_or_else(|e|e.into_inner())=json!({"enabled":true,"servers":present.len(),"players":last_players.iter().filter(|(id,_)|present.contains(*id)).map(|(_,n)|n).sum::<usize>(),"tiers":tiers,"active":in_flight.len(),"behind":behind,"stuck":flight_started.values().filter(|t|now-**t>120000).count(),"launched":launched,"beatAt":now});
          if now-settings_at>=10000{settings_at=now;if let Ok(next)=crate::settings::load(&app.db).await{settings=next;}}
          if now-roster_at>=5000{
           roster_at=now;
           let rows:Vec<(String,String,String,String)>=match sqlx::query_as("SELECT s.id,s.org_id,s.name,md5(s.host||':'||s.port::text||':'||s.scheme||':'||s.password_enc||':'||s.allow_private::text) FROM servers s JOIN organizations o ON o.id=s.org_id WHERE o.suspended_at IS NULL ORDER BY s.sort_order,s.name,s.id").fetch_all(&app.db).await{Ok(rows)=>rows,Err(_)=>{tracing::warn!("Server roster unavailable; pausing observations");continue}};
           present=rows.iter().map(|r|r.0.clone()).collect();memories.retain(|id,_|present.contains(id));signature.retain(|id,_|present.contains(id));
           for (id,org,name,fingerprint) in rows{
            let changed=signature.get(&id).is_some_and(|old|old!=&fingerprint);signature.insert(id.clone(),fingerprint);
            if let Some(m)=memories.get_mut(&id){m.org=org;m.name=name;if changed{wake(m,&Wake{identity:true,lists:true});}}
            else if in_flight.contains_key(&id){if changed{wakes.insert(id,Wake{identity:true,lists:true});}}
            else{memories.insert(id.clone(),Memory::new(id,org,name,now,number(&settings,"idleMs",30000)));}
           }
           // Expiry is a database operation, atomically fenced like the observations.
           let expired=match crate::list_sync::expire_entries(&app).await{Ok(value)=>value,Err(e) if e.code=="worker_ownership_lost"=>return Err(e),Err(_)=>{tracing::warn!("List expiry pass unavailable");json!({})}};
           if expired["lifted"].as_u64().unwrap_or(0)>0{for m in memories.values_mut(){if expired["orgIds"].as_array().is_some_and(|ids|ids.contains(&json!(m.org))){wake(m,&Wake{lists:true,identity:false});}}}
           roster_at=now;
          }
          let total=number(&settings,"concurrency",8).clamp(1,4096) as usize;
          let mut free=total.saturating_sub(in_flight.len());let offline=in_flight.values().filter(|off|**off).count();let mut offline_free=(total/4).max(1).saturating_sub(offline);
          let mut due=memories.values().filter(|m|m.players_due.min(m.status_due).max(m.hold)<=now).map(|m|(m.id.clone(),m.players_due.min(m.status_due),m.failures>=3)).collect::<Vec<_>>();due.sort_by(|a,b|a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
          for (id,_,offline) in due{
           if free==0{break}if offline&&offline_free==0{continue}free-=1;if offline{offline_free-=1}
           let mut m=memories.remove(&id).unwrap();let status_due=m.status_due<=now;let players_due=m.players_due<=now;
           let (pc,sc)=m.cadence(&settings,m.tier(&app));m.players_interval=pc;
           if players_due{m.players_due=observation_state::next_due(m.players_due,pc,now).max(m.hold)}if status_due{m.status_due=observation_state::next_due(m.status_due,sc,now).max(m.hold)}
           flight_started.insert(id.clone(),now);launched+=1;in_flight.insert(id,offline);let app=app.clone();let settings=settings.clone();
           tasks.spawn(async move{
            let result=async{
             let began=std::time::Instant::now();
             {let _lane=dispatcher::acquire(&m.id,2,Duration::from_secs(30)).await?;app.runtime.check().await?;observer::observe(&app,&mut m,status_due,players_due,&settings).await?;}
             crate::diagnostics::observation(m.ok,began.elapsed().as_secs_f64());
             if m.ok&&observer::now()-m.sync_at>=number(&settings,"listSyncMs",60000){let summary=crate::list_sync::reconcile(&app,&m.id,2).await?;if summary["ok"]==true{m.sync_at=observer::now();}}
             Ok(())
            }.await;
            (m,result)
           });
          }
         }
        }
    }
    // Drain existing operations; cancellation prevents any further game call or DB write.
    if tokio::time::timeout(Duration::from_secs(20), async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    Ok(())
}

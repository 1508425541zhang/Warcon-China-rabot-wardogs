import PostalMime from "postal-mime";

interface SendEmail {
  send(message: {
    to: string;
    from: string;
    subject: string;
    text?: string;
    html?: string;
  }): Promise<unknown>;
}

interface Env {
  DB: D1Database;
  EMAIL: SendEmail;
  APP_NAME: string;
  REPORT_FROM_EMAIL: string;
  ADMIN_EMAIL: string;
  ADMIN_TOKEN: string;
}

type ReportInput = {
  playerId?: unknown;
  category?: unknown;
  details?: unknown;
  reporterEmail?: unknown;
  website?: unknown;
};

const CATEGORIES = new Set(["作弊疑似", "破坏对局", "恶意交流", "利用漏洞", "其他"]);
const STATUS = new Set(["new", "reviewing", "closed"]);

const securityHeaders = {
  "content-security-policy":
    "default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
  "referrer-policy": "no-referrer",
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY",
  "permissions-policy": "camera=(), microphone=(), geolocation=()"
};

function json(data: unknown, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...securityHeaders }
  });
}

function text(value: unknown, max: number) {
  return typeof value === "string" ? value.trim().slice(0, max) : "";
}

function validEmail(value: string) {
  return !value || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value);
}

function authorized(request: Request, env: Env) {
  const auth = request.headers.get("authorization") || "";
  return !!env.ADMIN_TOKEN && auth === `Bearer ${env.ADMIN_TOKEN}`;
}

async function notifyAdmin(env: Env, report: {
  id: string;
  playerId: string;
  category: string;
  details: string;
  reporterEmail: string;
  createdAt: string;
}) {
  if (!env.ADMIN_EMAIL || !env.REPORT_FROM_EMAIL) return;
  const safeDetails = report.details.replace(/[<>]/g, "");
  await env.EMAIL.send({
    from: env.REPORT_FROM_EMAIL,
    to: env.ADMIN_EMAIL,
    subject: `[${env.APP_NAME || "战斗 Report"}] 新举报 ${report.category} / ${report.playerId}`,
    text:
      `举报编号: ${report.id}\n` +
      `目标: ${report.playerId}\n` +
      `分类: ${report.category}\n` +
      `时间: ${report.createdAt}\n` +
      `举报者邮箱: ${report.reporterEmail || "未填写"}\n\n` +
      report.details,
    html:
      `<h2>新举报</h2><p><b>编号：</b>${report.id}</p><p><b>目标：</b>${report.playerId}</p>` +
      `<p><b>分类：</b>${report.category}</p><p><b>时间：</b>${report.createdAt}</p>` +
      `<p><b>举报者邮箱：</b>${report.reporterEmail || "未填写"}</p><hr><p>${safeDetails.replace(/\n/g, "<br>")}</p>`
  });
}

async function createReport(request: Request, env: Env) {
  let input: ReportInput;
  try {
    input = await request.json<ReportInput>();
  } catch {
    return json({ ok: false, error: "JSON 格式无效" }, 400);
  }

  if (text(input.website, 200)) return json({ ok: true, id: crypto.randomUUID() }, 201);

  const playerId = text(input.playerId, 120);
  const category = text(input.category, 40);
  const details = text(input.details, 1200);
  const reporterEmail = text(input.reporterEmail, 160).toLowerCase();

  if (playerId.length < 2) return json({ ok: false, error: "请填写玩家名、玩家 ID 或 SteamID64" }, 400);
  if (!CATEGORIES.has(category)) return json({ ok: false, error: "举报分类无效" }, 400);
  if (details.length < 10) return json({ ok: false, error: "请至少用 10 个字描述发生了什么" }, 400);
  if (!validEmail(reporterEmail)) return json({ ok: false, error: "邮箱格式不正确" }, 400);

  const id = crypto.randomUUID();
  const createdAt = new Date().toISOString();

  await env.DB.prepare(
    "INSERT INTO reports (id, player_id, category, details, reporter_email, status, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'new', 'web', ?6)"
  )
    .bind(id, playerId, category, details, reporterEmail || null, createdAt)
    .run();

  try {
    await notifyAdmin(env, { id, playerId, category, details, reporterEmail, createdAt });
  } catch (error) {
    console.error("admin email failed", error);
  }

  return json({ ok: true, id, status: "new" }, 201);
}

async function listReports(request: Request, env: Env) {
  if (!authorized(request, env)) return json({ ok: false, error: "unauthorized" }, 401);

  const url = new URL(request.url);
  const limit = Math.max(1, Math.min(100, Number(url.searchParams.get("limit") || 50)));
  const status = text(url.searchParams.get("status"), 20);

  let stmt;
  if (status && STATUS.has(status)) {
    stmt = env.DB.prepare(
      "SELECT id, player_id AS playerId, category, details, reporter_email AS reporterEmail, status, source, created_at AS createdAt FROM reports WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2"
    ).bind(status, limit);
  } else {
    stmt = env.DB.prepare(
      "SELECT id, player_id AS playerId, category, details, reporter_email AS reporterEmail, status, source, created_at AS createdAt FROM reports ORDER BY created_at DESC LIMIT ?1"
    ).bind(limit);
  }

  const result = await stmt.run();
  return json({ ok: true, reports: result.results });
}

async function updateReport(request: Request, env: Env, id: string) {
  if (!authorized(request, env)) return json({ ok: false, error: "unauthorized" }, 401);

  let body: { status?: unknown };
  try {
    body = await request.json<{ status?: unknown }>();
  } catch {
    return json({ ok: false, error: "JSON 格式无效" }, 400);
  }

  const status = text(body.status, 20);
  if (!STATUS.has(status)) return json({ ok: false, error: "status 无效" }, 400);

  const result = await env.DB.prepare("UPDATE reports SET status = ?1 WHERE id = ?2")
    .bind(status, id)
    .run();

  if (!result.meta.changes) return json({ ok: false, error: "not_found" }, 404);
  return json({ ok: true, id, status });
}

async function health(env: Env) {
  const row = await env.DB.prepare("SELECT COUNT(*) AS n FROM reports").first<{ n: number }>();
  return json({ ok: true, service: env.APP_NAME || "战斗 Report", reports: Number(row?.n || 0) });
}

const page = `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="theme-color" content="#080b0d">
<title>战斗 Report</title>
<style>
:root{color-scheme:dark;--lime:#cfff43;--cyan:#65e7f6;--red:#ff646b;--bg:#070a0b;--muted:#8b969c}
*{box-sizing:border-box}body{margin:0;background:radial-gradient(circle at 16% -10%,#1b2a13 0,transparent 34%),radial-gradient(circle at 100% 20%,#0e2730 0,transparent 29%),var(--bg);font-family:Inter,system-ui,-apple-system,"Segoe UI",sans-serif;color:#eef2f3;min-height:100vh}
body:before{content:"";position:fixed;inset:0;pointer-events:none;opacity:.08;background-image:linear-gradient(#fff 1px,transparent 1px),linear-gradient(90deg,#fff 1px,transparent 1px);background-size:48px 48px;mask-image:linear-gradient(#000,transparent 72%)}
.wrap{width:min(1180px,calc(100% - 28px));margin:auto;padding:28px 0 52px}.top{display:flex;justify-content:space-between;align-items:center;border-bottom:1px solid #ffffff12;padding:0 0 18px}.brand{font:800 14px/1 ui-monospace,SFMono-Regular,monospace;letter-spacing:.16em;text-transform:uppercase}.brand b{color:var(--lime)}.online{font:700 11px/1 ui-monospace,monospace;color:var(--lime);letter-spacing:.1em}
.hero{display:grid;grid-template-columns:1.05fr .95fr;gap:34px;align-items:center;padding:72px 0 42px}.eyebrow{font:700 11px/1.3 ui-monospace,monospace;color:#cfff43b0;letter-spacing:.18em}.title{margin:16px 0 20px;font-size:clamp(68px,11vw,138px);line-height:.76;letter-spacing:-.06em;font-weight:900}.title span{display:block;color:transparent;-webkit-text-stroke:1.5px var(--lime)}.lead{max-width:680px;color:#c5ced2b8;font-size:17px;line-height:1.8}.notice{margin-top:22px;border-left:2px solid var(--lime);background:linear-gradient(90deg,#cfff4312,transparent);padding:14px 16px;color:#aeb9be;font-size:13px;line-height:1.65}.notice b{color:#fff}
.radarbox{min-height:430px;border:1px solid #cfff4329;border-radius:22px;background:#07100dcc;display:grid;place-items:center;position:relative;overflow:hidden}.radarbox:before{content:"EVIDENCE / COMMUNITY / REVIEW";position:absolute;top:18px;left:20px;font:700 10px ui-monospace,monospace;letter-spacing:.16em;color:#ffffff51}.radar{width:min(74%,340px);aspect-ratio:1;border:1px solid #cfff4352;border-radius:50%;position:relative;background:repeating-radial-gradient(circle,transparent 0 18%,#cfff4317 18.5% 19%,transparent 19.5% 36%),linear-gradient(90deg,transparent 49.8%,#cfff4320 50%,transparent 50.2%),linear-gradient(transparent 49.8%,#cfff4320 50%,transparent 50.2%);box-shadow:inset 0 0 80px #cfff4312}.radar:before{content:"";position:absolute;inset:-1px;border-radius:50%;background:conic-gradient(transparent 0 312deg,#cfff434c 349deg,transparent 360deg);animation:spin 4.2s linear infinite}.radar:after{content:"";position:absolute;inset:42%;border:1px solid #fff7;border-radius:50%;box-shadow:0 0 0 22px #ffffff08,0 0 22px #cfff436e}.blip{position:absolute;width:8px;height:8px;border-radius:50%;background:var(--lime);box-shadow:0 0 16px var(--lime)}.b1{left:66%;top:29%}.b2{left:31%;top:67%}.b3{left:77%;top:57%;background:var(--red);box-shadow:0 0 16px var(--red)}
.panelgrid{display:grid;grid-template-columns:.72fr 1.28fr;gap:16px}.info,.form{border:1px solid #ffffff13;border-radius:18px;background:#ffffff06;backdrop-filter:blur(14px);padding:26px}.kicker{font:700 10px ui-monospace,monospace;letter-spacing:.17em;color:#ffffff68;text-transform:uppercase}.info h2,.form h2{font-size:30px;margin:8px 0 14px}.info p{color:#aab4b9;font-size:14px;line-height:1.7}.steps{display:grid;gap:12px;margin-top:22px}.step{display:grid;grid-template-columns:28px 1fr;gap:10px;border-top:1px solid #ffffff12;padding-top:12px}.step i{font:700 11px ui-monospace,monospace;color:var(--lime);font-style:normal}.step b{font-size:13px}.step small{display:block;color:#7f8a90;line-height:1.5;margin-top:4px}
form{display:grid;gap:18px}.row{display:grid;gap:8px}.rowhead{display:flex;justify-content:space-between;gap:12px;align-items:center}.rowhead label{font-size:13px;font-weight:800}.rowhead span{font:600 10px ui-monospace,monospace;color:#ffffff4a;letter-spacing:.1em}input,textarea{width:100%;border:1px solid #ffffff18;border-radius:11px;background:#0005;color:#fff;padding:14px 15px;outline:0;font:inherit}textarea{min-height:150px;resize:vertical;line-height:1.6}input:focus,textarea:focus{border-color:#cfff437a;box-shadow:0 0 0 3px #cfff430d}.cats{display:grid;grid-template-columns:repeat(5,1fr);gap:8px}.cat{border:1px solid #ffffff13;background:#ffffff05;color:#b9c3c7;border-radius:10px;padding:12px 8px;cursor:pointer;font-weight:800;font-size:12px}.cat.active{border-color:#cfff4370;color:var(--lime);background:#cfff4310}.help{font-size:11px;color:#737f85;line-height:1.55}.submit{display:flex;justify-content:space-between;gap:18px;align-items:center;border-top:1px solid #ffffff10;padding-top:18px}.submit p{margin:0;color:#778289;font-size:11px;line-height:1.55}.btn{border:0;border-radius:10px;background:var(--lime);color:#091006;font-weight:900;padding:14px 21px;cursor:pointer;min-width:150px}.btn:disabled{opacity:.55;cursor:wait}.status{display:none;margin-top:14px;padding:14px 16px;border-radius:10px;font-size:13px;line-height:1.55}.status.show{display:block}.status.ok{border:1px solid #cfff4337;background:#cfff430d;color:#dfffb0}.status.err{border:1px solid #ff646b42;background:#ff646b0c;color:#ffadb1}.hp{position:absolute;left:-9999px}
footer{border-top:1px solid #ffffff10;margin-top:26px;padding-top:18px;color:#626d72;font-size:11px;display:flex;justify-content:space-between;gap:18px}
@keyframes spin{to{transform:rotate(360deg)}}@media(max-width:850px){.hero,.panelgrid{grid-template-columns:1fr}.hero{padding-top:48px}.radarbox{min-height:360px}.cats{grid-template-columns:repeat(2,1fr)}}@media(max-width:520px){.wrap{width:min(100% - 18px,1180px)}.title{font-size:68px}.info,.form{padding:18px}.submit{align-items:stretch;flex-direction:column}.btn{width:100%}.cats{grid-template-columns:1fr}footer{flex-direction:column}}@media(prefers-reduced-motion:reduce){*{animation:none!important;scroll-behavior:auto!important}}
</style>
</head>
<body>
<main class="wrap">
<header class="top"><div class="brand">战斗 <b>REPORT</b></div><div class="online">● WORKER ONLINE</div></header>
<section class="hero">
<div>
<div class="eyebrow">COMMUNITY REPORTING / CLOUDFLARE WORKER</div>
<h1 class="title">战斗<span>REPORT</span></h1>
<p class="lead">一个独立运行的社区举报入口。网页、接口、数据库和邮件处理全部运行在 Cloudflare Worker 上，不依赖传统服务器。</p>
<div class="notice"><b>举报不是定罪。</b> 举报只会进入审核数据库并通知管理员，不会因为次数或文字描述自动处罚玩家。</div>
</div>
<div class="radarbox"><div class="radar"><i class="blip b1"></i><i class="blip b2"></i><i class="blip b3"></i></div></div>
</section>
<section class="panelgrid">
<aside class="info">
<div class="kicker">REPORT PROTOCOL</div><h2>把情况写成可复核记录</h2>
<p>尽量写清具体时间、局次、行为和重复情况。不要提交住址、电话等现实身份信息。</p>
<div class="steps">
<div class="step"><i>01</i><div><b>目标准确</b><small>玩家名、游戏 ID 或 SteamID64。</small></div></div>
<div class="step"><i>02</i><div><b>描述事实</b><small>描述你实际看到的行为，而不是只写结论。</small></div></div>
<div class="step"><i>03</i><div><b>人工审核</b><small>数据保存到 D1，管理员收到邮件提醒后复核。</small></div></div>
</div>
</aside>
<section class="form">
<div class="kicker">NEW CASE</div><h2>新建举报</h2>
<form id="reportForm">
<div class="row"><div class="rowhead"><label for="playerId">玩家 / ID</label><span>PLAYER TARGET</span></div><input id="playerId" maxlength="120" placeholder="玩家名、游戏 ID 或 SteamID64" required></div>
<div class="row"><div class="rowhead"><label>举报分类</label><span>REASON TYPE</span></div><div class="cats" id="cats"></div></div>
<div class="row"><div class="rowhead"><label for="details">发生了什么</label><span id="counter">0 / 1200</span></div><textarea id="details" maxlength="1200" minlength="10" placeholder="例如：第 4 回合约 02:10，目标在没有视野的情况下连续预瞄两名玩家；第 6 回合出现了相同情况。请复核对应时间段。" required></textarea><div class="help">只提交与游戏行为相关的信息。请不要填写现实世界的敏感个人信息。</div></div>
<div class="row"><div class="rowhead"><label for="reporterEmail">你的邮箱（可选）</label><span>CONTACT</span></div><input id="reporterEmail" type="email" maxlength="160" placeholder="用于管理员需要补充信息时联系你"></div>
<label class="hp">Website<input id="website" tabindex="-1" autocomplete="off"></label>
<div class="submit"><p>提交后写入 Cloudflare D1。Worker 会尝试向管理员发送新举报通知邮件。</p><button class="btn" id="submitBtn" type="submit">提交举报 →</button></div>
</form>
<div class="status" id="status" role="status"></div>
</section>
</section>
<footer><span>战斗 Report / Worker + D1 + Email</span><span>Privacy-first community reporting</span></footer>
</main>
<script>
const categories=["作弊疑似","破坏对局","恶意交流","利用漏洞","其他"];let selected=categories[0];
const cats=document.querySelector("#cats");const statusEl=document.querySelector("#status");const details=document.querySelector("#details");
function renderCats(){cats.innerHTML="";categories.forEach(c=>{const b=document.createElement("button");b.type="button";b.className="cat"+(c===selected?" active":"");b.textContent=c;b.onclick=()=>{selected=c;renderCats()};cats.appendChild(b)})}
renderCats();details.addEventListener("input",()=>document.querySelector("#counter").textContent=details.value.length+" / 1200");
document.querySelector("#reportForm").addEventListener("submit",async e=>{e.preventDefault();const btn=document.querySelector("#submitBtn");btn.disabled=true;statusEl.className="status";try{const res=await fetch("/api/report",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({playerId:document.querySelector("#playerId").value,category:selected,details:details.value,reporterEmail:document.querySelector("#reporterEmail").value,website:document.querySelector("#website").value})});const data=await res.json();if(!res.ok||!data.ok)throw new Error(data.error||"提交失败");statusEl.className="status show ok";statusEl.textContent="举报已保存。案件编号："+data.id+"。请保留这个编号。";e.target.reset();details.dispatchEvent(new Event("input"));selected=categories[0];renderCats()}catch(err){statusEl.className="status show err";statusEl.textContent=err.message||"提交失败，请稍后重试"}finally{btn.disabled=false}});
</script>
</body>
</html>`;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "GET" && url.pathname === "/") {
      return new Response(page, {
        headers: { "content-type": "text/html; charset=utf-8", ...securityHeaders }
      });
    }

    if (request.method === "GET" && url.pathname === "/api/health") return health(env);
    if (request.method === "POST" && url.pathname === "/api/report") return createReport(request, env);
    if (request.method === "GET" && url.pathname === "/api/reports") return listReports(request, env);

    const match = /^\/api\/reports\/([0-9a-f-]{36})$/.exec(url.pathname);
    if (request.method === "PATCH" && match) return updateReport(request, env, match[1]);

    return json({ ok: false, error: "not_found" }, 404);
  },

  async email(message: ForwardableEmailMessage, env: Env): Promise<void> {
    const parser = new PostalMime();
    let body = "";
    try {
      const parsed = await parser.parse(await new Response(message.raw).arrayBuffer());
      body = (parsed.text || "").slice(0, 20000);
    } catch (error) {
      console.error("email parse failed", error);
    }

    const id = crypto.randomUUID();
    const createdAt = new Date().toISOString();
    const subject = (message.headers.get("subject") || "").slice(0, 300);
    const messageId = (message.headers.get("message-id") || "").slice(0, 500);

    await env.DB.prepare(
      "INSERT INTO inbound_emails (id, mail_from, rcpt_to, subject, text_body, message_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
    )
      .bind(id, message.from, message.to, subject, body, messageId || null, createdAt)
      .run();

    try {
      await message.reply({
        from: message.to,
        to: message.from,
        subject: subject ? `Re: ${subject}` : `${env.APP_NAME || "战斗 Report"}：邮件已收到`,
        text:
          `你的邮件已经被 ${env.APP_NAME || "战斗 Report"} 收到并进入记录。\n\n` +
          `邮件记录编号：${id}\n` +
          "本回执仅确认收件，不代表举报内容已经被认定成立。"
      } as never);
    } catch (error) {
      console.error("email reply failed", error);
    }
  }
} satisfies ExportedHandler<Env>;

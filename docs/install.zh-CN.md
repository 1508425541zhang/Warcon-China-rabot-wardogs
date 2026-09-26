# Warcon China：从零安装

这篇按“先在自己的电脑上看到网页，再接入游戏服”的顺序写。**Docker Compose 会启动 PostgreSQL/TimescaleDB，不需要另外安装 PostgreSQL，也不需要在电脑上安装 Bun。**新安装没有预设的网页登录账号和密码：第一个所有者账号由你在 `/setup` 自己创建。

## 第 0 步：准备电脑

先用自己的电脑体验：Windows 安装并打开 [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/)（按安装程序提示启用 WSL 2）；macOS 安装并打开 [Docker Desktop](https://docs.docker.com/desktop/setup/install/mac-install/)；Ubuntu 按[官方步骤安装 Docker Engine 和 Compose 插件](https://docs.docker.com/engine/install/ubuntu/)。再安装 [Git](https://git-scm.com/downloads)。

Windows 在开始菜单打开 **PowerShell**；macOS/Linux 打开 **终端**。逐行运行：

```text
docker --version
docker compose version
git --version
```

前三条各显示版本号后再继续。如果 `docker` 找不到，重新打开终端；如果提示无法连接 Docker daemon，先启动 Docker Desktop。Ubuntu 上如果提示 Docker 权限不足，按官方文档配置用户权限，或在下面的 Docker 命令前加 `sudo`。

## 第 1 步：把项目下载到电脑

在终端依次执行：

```text
git clone https://github.com/1508425541zhang/Warcon-China-rabot-wardogs.git
cd Warcon-China-rabot-wardogs
```

运行 `ls`，确认当前目录里有 `docker-compose.yml` 和 `.env.example`。**从现在起，本文后面的命令都在这个目录执行。**不想装 Git 时，也可以在 GitHub 仓库首页点 **Code → Download ZIP**，解压后在解压出的项目目录打开终端。

## 第 2 步：复制配置模板

Windows PowerShell：

```powershell
Copy-Item .env.example .env
notepad .env
```

macOS/Linux：

```bash
cp .env.example .env
nano .env
```

现在编辑器里打开的 `.env` 是你的私有配置。找到下面五个名字，只改等号右边：

| 名字 | 本机体验时填什么 | 是什么 |
| --- | --- | --- |
| `BETTER_AUTH_SECRET` | 随机字符串 A | 登录会话密钥 |
| `ENCRYPTION_KEY` | 随机字符串 B | 加密游戏服 RCON 密码的密钥，以后不要随意更换 |
| `RELAY_SECRET` | 随机字符串 C | 网页和 Worker 的内部通信密钥 |
| `POSTGRES_PASSWORD` | 随机字符串 D | 数据库密码，**不是网页登录密码** |
| `ORIGIN` | `http://localhost:3000` | 你稍后在浏览器打开的精确地址 |

**A、B、C、D 每个都必须不同。**Windows PowerShell 运行下面整段四次，每次把显示的结果复制到不同设置后面：

```powershell
$bytes = New-Object byte[] 32
$rng = [Security.Cryptography.RandomNumberGenerator]::Create()
$rng.GetBytes($bytes)
[Convert]::ToBase64String($bytes)
$rng.Dispose()
```

macOS/Linux 运行下面一行四次：

```bash
openssl rand -base64 32
```

不要真的填写字母 `A`、`B`、`C`、`D`，也不要沿用示例里的 `change-me` 或 `replace-with-...`。等号两侧不要加空格。记事本按 `Ctrl+S` 保存；`nano` 按 `Ctrl+O`、回车、`Ctrl+X` 保存退出。**不要把 `.env` 提交到 GitHub，也不要把它发给别人。**使用本指南的内置数据库时，不要取消 `DATABASE_URL` 那行的注释。

## 第 3 步：检查配置并启动

先运行：

```text
docker compose config -q
```

没有输出就表示 Compose 配置能解析。如果报缺少 `POSTGRES_PASSWORD`，回到第 2 步检查 `.env` 是否保存、这一行是否留空。然后运行：

```text
docker compose up -d --build
```

第一次要下载数据库镜像、构建应用，可能需要几分钟。命令结束后检查：

```text
docker compose ps -a
```

预期结果：`db` 显示 **healthy**，`warcon` 和 `worker` 显示 **running**；`migrate` 显示 **exited (0)** 是正常的，因为它只负责更新数据库结构。浏览器直接打开 `http://localhost:3000/api/health` 也应该得到正常响应。

这里的四项分别是数据库 `db`、数据库迁移 `migrate`、网页 `warcon`、处理游戏服事件的 `worker`。浏览器不会直接连接游戏 RCON。

## 第 4 步：创建第一个网页登录账号

在运行 Docker 的**同一台电脑**打开浏览器，输入：

```text
http://localhost:3000/setup
```

在页面里**自己选择**用户名和强密码，保存到你的密码管理器。这是站点所有者账号。以后用 `http://localhost:3000/sign-in` 登录。`POSTGRES_PASSWORD` 无法用于网页登录。如果你额外设置了 `SETUP_TOKEN`，首次设置页面还会要求输入它。

如果 `/setup` 自动跳到 `/sign-in`，说明这份数据库已经有账号，不能再创建第一个账号。见下文“忘记登录密码”。

## 第 5 步：先用模拟服务器看界面

登录后，创建或打开一个组织，然后进入 **服务器 → 添加服务器**。按下表填：

| 字段 | 值 |
| --- | --- |
| 名称 | `演示服务器` |
| 主机地址 | `demo` |
| 端口 | `1` |
| 协议 | `http` |
| RCON 密码 | `demo` |

主机地址和密码里的 `demo` 都必须是英文。保存后即可看面板，不需要真实游戏服。模拟数据不等于真实玩家历史：统计风控基线样本不足时会显示 `INSUFFICIENT_DATA`，不会编造分位数或因此自动处置。

## 第 6 步：有游戏服后再接入

向游戏服提供者取得**真实的 RCON 主机地址、端口和密码**。在游戏服的 `ServerSettings.ini` 启用对应的 WARDOGS RCON 设置并重启游戏服；端口常见为 `7776`，但以你实际配置为准。到面板的 **服务器 → 添加服务器** 填写真实信息，先点 **测试**，成功后保存。完整的游戏服配置示例见[上游接入说明](getting-started.md#turning-on-rcon-on-the-game-server)。

运行面板的机器必须能连到游戏服 RCON 端口。只允许这台机器访问该端口，不要把 RCON 公开给整个互联网。如果游戏服与 Docker 在同一台机器上，容器里的 `127.0.0.1` 不是宿主机；这时按 `docker-compose.yml` 的 `extra_hosts` 注释及[上游接入说明](getting-started.md#turning-on-rcon-on-the-game-server)使用 `host.docker.internal`。

### 6.1 单独启用 Kill Feed：这一步决定能否收到击杀

RCON 测试成功只说明“面板能连接游戏服”。Kill Feed 的方向相反：**游戏服 → 面板公网 HTTPS 地址**。两个连接都成功，才有完整数据。

1. 先完成下文“准备开放给互联网时”的域名、HTTPS 与 `ORIGIN` 设置。游戏服在另一台机器时，不能填 `localhost`、`127.0.0.1` 或只能在你家访问的地址。
2. 登录面板，打开真实服务器的 **配置 → 击杀事件**。组织所有者点击 **配置**。
3. 面板生成专用令牌，并把回传配置写入 `ServerSettings.ini` 的 `[WDServerFeed]`。核对页面显示的地址与令牌；游戏会自行追加 `/api/ingest/events`，不要擅自重复追加路径。
4. 如果配置文件不可写，先让游戏服提供者解决配置写入权限；如果值由游戏启动参数固定，需要修改游戏服启动参数，网页无法覆盖。
5. 写入后在合适时间重启**游戏服务器**，使游戏读取配置。只重启 Warcon 容器不能让游戏重新读取其配置。
6. 产生一次真实击杀，再看页面是否从“尚未收到任何批次”变为最近收到事件。已生成令牌不等于已经收到事件。
7. 更换令牌或直接修改过游戏服文件时，点击 **再次写入配置** 并让游戏重新读取；旧令牌不能继续使用。

如果没有批次，检查游戏服能否访问面板 HTTPS、证书是否可信、反向代理是否允许 POST `/api/ingest/events`、防火墙是否放行。已有批次却无纯步兵 KPM，再检查武器分类、双方阵营和玩家列表是否及时；换图本身不能修复缺失来源。

### 6.2 开启需要的功能

社区风控默认是**统计影子模式**：同时保存旧评分和真实历史百分位，实际自动处置仍由旧规则决定；自动处置开关初始关闭。统计基线需要最近 30 天足够多的有效纯步兵击杀事件。没有足够样本时页面会明确说明，而不是显示假的曲线。

统计委员会规则见[五专家 v3](committee-v3.zh-CN.md)。默认影子模式不等于委员会自动执行；切换统计模式及开启自动踢出前，应检查页面的数据健康、基线与保护状态。普通投票只建案件，KPM＞4 且另一独立专家至少可疑才可能直接踢出。

可选设置：

- Steam 公开资料：将自己的 `STEAM_API_KEY` 写入 `.env`，再运行 `docker compose up -d`。没有 Key 仍可管理服务器；Steam Key 无法补回击杀日志。
- AI 辅审：进入风控 AI 设置，填写兼容服务的地址、密钥和模型，测试成功后启用自动审查，见[AI 指南](integrity-ai.zh-CN.md)。
- 自动化：逐项开启需要的规则。赛后荣誉还须启用对局广播规则及“自动生成本局获奖名单并广播”，见[模板指南](match-awards.zh-CN.md)。

## 第 7 步：日常操作

每次先确认 Docker Desktop 正在运行。下面的命令仍在项目目录执行：

| 目的 | 命令 |
| --- | --- |
| 查看服务 | `docker compose ps -a` |
| 看网页日志 | `docker compose logs --tail=80 warcon` |
| 看 Worker 日志 | `docker compose logs --tail=80 worker` |
| 停止，保留数据 | `docker compose down` |
| 再启动 | `docker compose up -d` |
| 更新代码和容器 | `git pull --ff-only origin main`，随后 `docker compose up -d --build`（先按下文备份） |

**不要执行 `docker compose down -v`**：`-v` 会删除数据库卷。更新前备份 `.env` 和数据库；具体备份命令见下一节。

### 7.1 备份：先留住数据，再更新

以下命令使用 Compose 内置数据库，先在项目目录执行。备份文件可能含玩家与审核数据，应保存到只有管理员能访问的位置。

```text
docker compose exec -T db pg_dump -U warcon -d warcon -Fc -f /tmp/warcon-backup.dump
docker compose cp db:/tmp/warcon-backup.dump ./warcon-backup.dump
```

确认当前目录出现非空 `warcon-backup.dump`，改成带日期的文件名另存，避免下一次覆盖。另行安全备份 `.env`，尤其是 `ENCRYPTION_KEY`；只有数据库、没有原加密密钥，将无法解密已有 RCON 等凭据。这种容器内生成再复制的方法也适用于 Windows PowerShell，不依赖二进制输出重定向。

需要恢复时，先保留现有部署与数据，在独立目录／独立 Compose 项目准备恢复环境，使用匹配数据库版本和原 `.env`，将备份交给 `pg_restore` 恢复后核验。不要对唯一生产库直接清空恢复；完整运维参考[上游说明](../README.upstream.md)。

### 7.2 更新与重启

用 Git 克隆安装且本地没有改动时：

```text
git pull --ff-only origin main
docker compose up -d --build
docker compose ps -a
docker compose logs --tail=80 migrate warcon worker
```

如果 Git 提示有本地修改或分支不能快进，先保留修改并解决差异，不要直接强制重置。下载 ZIP 安装的用户可解压新版本到新目录，保留原配置和数据库，避免因项目目录名改变而误连一个新空卷。数据库迁移可能不可逆，回退镜像并不等于回退数据库。

服务设置了 `restart: unless-stopped`；主机重启后仍需要 Docker 引擎启动。Windows 在 Docker Desktop 设置中启用登录后自动启动，电脑不要休眠；关机和休眠时不会继续处理服务器事件。长期运行可部署在持续开机的 Linux 主机上。

### 7.3 本机 3000 端口被占用

停止当前服务后，在 `docker-compose.yml` 将已有的 `'3000:3000'` 改为 `'3300:3000'`，只修改左边的宿主机端口；把 `.env` 改为 `ORIGIN=http://localhost:3300`。运行 `docker compose up -d`，之后访问 `http://localhost:3300`。不要再使用旧的 3000 地址。

## 出问题时按顺序检查

1. `docker` 找不到或无法连接：确认 Docker 已安装并启动，重新打开终端，再跑第 0 步的版本检查。
2. 启动失败：运行 `docker compose ps -a`，再运行 `docker compose logs --tail=80 migrate warcon worker db`。`migrate` 必须成功退出。
3. 浏览器打不开：确认第 3 步的启动命令已结束，`warcon` 正在运行，且本机 `3000` 端口没有被其他程序占用。
4. 登录页面能打开却无法登录：浏览器地址必须与 `.env` 中的 `ORIGIN` 完全一致；`localhost` 与 `127.0.0.1` 不要混用。修改 `.env` 后重新运行 `docker compose up -d`。
5. `/setup` 跳到 `/sign-in`：数据库已有所有者。网页登录密码是创建账号时设置的，不是数据库密码。
6. 游戏服“测试”失败：检查 RCON 地址、端口、密码和防火墙；同机部署还要检查第 6 步的容器网络地址。
7. 风控页面没有分布图：检查真实 Kill Feed 是否正常，以及有效历史样本是否已达到最低数量；`INSUFFICIENT_DATA` 是预期的安全状态。

### 忘记网页登录密码

有主机管理权限的人可为**已有用户名**生成一次性临时密码。下例的 `admin` 必须换成你实际创建的用户名。这个命令会退出该用户的旧会话，并清除其验证器、通行密钥和恢复密钥；先确认这一影响：

```text
docker compose run --rm worker bun ./build/reset-auth.js admin
```

终端只显示一次临时密码；保存好，在 `/sign-in` 登录后按提示修改。不要把它写进 README、提交到 GitHub 或发到公开频道。

## 准备开放给互联网时

按以下顺序配置公网部署：

1. 准备持续运行的主机和自己的域名，例如 `panel.example.com`；把域名 DNS 指向这台主机。
2. 用 Caddy、Nginx 等反向代理配置域名与可信 HTTPS 证书，将请求转发到 Warcon 的 3000 端口。代理与应用在不同容器时，要使用可互通的 Docker 网络地址，不能把代理容器的 `localhost` 当作应用。
3. 如果代理直接运行在宿主机，可把 Compose 已有端口改为 `'127.0.0.1:3000:3000'`，让浏览器和游戏服统一经过 HTTPS。默认 `'3000:3000'` 会监听所有网卡，请结合防火墙限制直连。
4. 设置 `.env` 中 `ORIGIN=https://panel.example.com`，运行 `docker compose up -d`。管理员始终用同一域名登录。
5. 按实际代理链配置客户端 IP 头，只信任你控制的代理；不要盲目开启任意来源的转发头。数据库与 Worker 内部端口无需公开。
6. 用外部网络打开登录页及 `/api/health`，再回第 6.1 节配置真实游戏服回传。代理不得用交互登录页或验证挑战阻断 Kill Feed 路径；该接口使用自己的令牌验证。

本机测试地址 `http://localhost:3000` 只适合同一台电脑。需要远程管理员登录时，先配置域名和 **HTTPS 反向代理**，再把 `.env` 的 `ORIGIN` 改成管理员实际打开的完整地址，例如 `https://panel.example.com`（末尾没有 `/`）。按[上游部署说明](../README.upstream.md)配置代理和客户端 IP 头。不要向互联网开放数据库与 Worker 的内部端口，并定期备份数据库和 `.env`。

本项目不是客户端反作弊软件，不扫描玩家设备。当前没有已验证的游戏聊天读取接口，因此游戏内 `!report`、`!BAN` 未接入；也没有自动永久封禁。更多信息见[技术架构](architecture.zh-CN.md)与[完整功能介绍](features.zh-CN.md)。

# WARCON 模型 HTTP 服务 · A测

Epoch 58 Anomaly Transformer 独立推理进程；与 WARCON 通过 HTTP 互通。

完整说明、输入协议、配置入口与 P95 踢出 / P99 隔离 24 小时规则见 [接入文档](../../docs/model-http-alpha.zh-CN.md)。

```sh
pip install -r requirements.txt
export MODEL_API_TOKEN='replace-with-your-own-long-random-token'
python server.py
```

默认仅监听 localhost:8091。Docker 构建：`docker build -t warcon-model-alpha .`。
本分支只构建，不上线，不合并 main。

`model/` 源于 THUML Anomaly-Transformer，保留 MIT LICENSE；CPU buffer 适配不改变模型权重。
`features.py` 复用原训练特征生成器，额外接受 `roster_size`，保证单玩家请求保留真实总人数。

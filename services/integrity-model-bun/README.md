# Epoch 58 模型 HTTP API（Bun 原生，A测）

本服务只需要 WARCON 已有的 Bun 1.4.2。无需 Python、PyTorch、NumPy、ONNX、Docker、npm 安装或其他推理依赖。生产目录只包含 TypeScript 和冻结模型数据；训练与格式转换在本机完成。

活动模型 `warcon-at-epoch58`，原始 checkpoint SHA256 `ea3bc3f168e7ca8e329e85736d74c59019d6392cbded31bc63bee41ad7378da7`。`weights.f32` 是原始 state_dict 的小端 float32 导出，`weights.json` 标明形状和偏移。位置编码只保留推理窗口需要的 200 步。manifest 的 checkpoint 字段记录训练来源，运行不读取 .pth。启动时校验权重、索引、scaler、校准文件哈希。没有重新训练或更换阈值。

推理包含循环卷积、两层四头全注意力、LayerNorm、erf GELU、重建输出和训练时相同的带掩码损失。只省略不参与重建评分的 sigma/prior 分支及 eval 禁用的 dropout。14 个数值通道与 13 个观测掩码保留原有缺失语义。四张原始表重建 30 秒桶；需要 200 连续活跃桶、80% 有观测、75% 现金覆盖，否则返回 `INSUFFICIENT_DATA`。

启动：

```sh
MODEL_API_TOKEN=<至少32字符的私密令牌> bun services/integrity-model-bun/server.ts
```

默认监听 `127.0.0.1:8091`。环境变量：`MODEL_HOST`、`MODEL_PORT`、`MODEL_ARTIFACTS_DIR`。Bearer 令牌通过私密环境文件传入，不能提交到 Git。可安装同目录的 `warcon-model.service`，部署源目录到 `/opt/warcon-model`，环境文件放 `/etc/warcon/model.env`（root:warcon 0640）。资源限制独立于 WARCON 主进程。

- `GET /v1/health`：模型、checkpoint、校准指纹和运行时。
- `POST /v1/assess`：输入 `{schema:"warcon-raw-30s-v1",requestId,sources:{matches,player_progress_samples,integrity_player_metric_history,integrity_windows}}`。仅允许单局、单玩家，各表最多 25000 条，总体最多 8 MiB。
- 两个接口都需要 `Authorization: Bearer <令牌>`。服务没有外部依赖、数据库访问或处罚权限。HTTP 请求经过 WARCON 后台，处罚由现有队列执行。

组织的“反作弊管理”页面提供模型开发者设置：地址填写 `http://127.0.0.1:8091`，令牌加密存储；选择“仅模型”后执行模型判断。固定 P95 踢出、P99 临时隔离 24 小时。VIP 白名单、数据健康检查、版本校验、去重、处罚冷却及每小时上限继续生效。未满足数据覆盖、HTTP 异常或版本错误不会产生处罚，也不会回退到专家结论。

这是异常重建分数，不能解释成作弊概率；校准来源是无标签训练参考分布。A测保留人工复核记录，不合并 main。

验证：`bun test services/integrity-model-bun/inference.test.ts`。`parity.json` 保存原 PyTorch Epoch58 的三种固定输入参考输出，覆盖完整/缺失掩码、较大数值，核对预测采样、200 个逐点评分和总体评分。另在本机核对四表重建与原服务逐项一致。原架构许可证见 `MODEL-LICENSE`。

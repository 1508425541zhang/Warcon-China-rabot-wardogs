# Epoch58 的30分钟时序模型（A测）

活动模型 `warcon-at-epoch58-30m-v1` 由原 Epoch58 适配。每30秒一个采样点，60个有序点组成30分钟序列；每玩家默认每1800秒评估一次。保留两层四头注意力、循环卷积、LayerNorm、GELU、原冻结scaler及14个数值通道和13个观测掩码。没有替换成单点模型，没有补齐或复制缺失时间点。

适配使用3838个完整训练窗口，以第24局的882个窗口选择第11轮。原始checkpoint的SHA256为 `ea3bc3f168e7ca8e329e85736d74c59019d6392cbded31bc63bee41ad7378da7`。活动冻结文件位于 `artifacts-30m`，原200步模型位于 `artifacts` 供回归核对。小端float32权重和形状索引支持Bun原生推理，无需Python、PyTorch、NumPy或ONNX。manifest记录原始和适配checkpoint、权重、索引、scaler和校准指纹，加载时验证完整性。

输入协议为 `{schema:"warcon-raw-30s-v1",requestId,sources:{matches,player_progress_samples,integrity_player_metric_history,integrity_windows}}`，每次限单局、单玩家。四张原始表重建30秒桶；需要60个连续活跃桶、80%观测和75%现金覆盖，末桶必须新鲜。换局重新累计，禁止跨局拼接。不足时返回 `INSUFFICIENT_DATA`、原因及覆盖数量；完整序列返回 `READY`、总体重建分数和60个逐点评分。定时调度无需玩家先产生击杀事件。

P95/P99按60步评分重新校准，参考第26局679个无标签窗口。P95自动踢出，P99优先隔离24小时；保留VIP白名单、观测健康检查、版本验证、去重、处罚冷却和每小时上限。请求失败或数据不足不产生处罚，不回退专家。后台显示覆盖情况和不足原因。

第27局750个窗口未参与原训练或本次适配、选择和校准；结果见 `artifacts-30m/evaluation.json`。第24/26局曾出现在Epoch58预训练中，本次适配验证和校准并不独立于预训练。窗口存在重叠，数量不等于独立样本数。没有作弊真值标签，不能报告准确率、召回率或认定超过百分位即作弊。

两套parity文件记录原200步和适配60步的PyTorch CPU参考输出，覆盖完整、缺失掩码和较大数值，验证预测、逐点评分和总体分数。原架构许可证见 `MODEL-LICENSE`。A测保持独立分支，不合并main。

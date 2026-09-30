export const qqProviders = {
	napcat: {
		name: 'NapCat',
		placeholder: 'http://127.0.0.1:3001',
		guide: 'https://napneko.github.io/config/basic',
		setup:
			'在 NapCat 网络配置中启用 HTTP 服务端和 HTTP 客户端。客户端填写上报地址和事件签名密钥，messagePostFormat 选择 array。'
	},
	llbot: {
		name: 'LLBot（LuckyLilliaBot）',
		placeholder: 'http://127.0.0.1:3002',
		guide: 'https://luckylillia.com/guide/config',
		setup:
			'在 LLBot 的 Bot 配置中启用 OneBot 11，添加 http 和 http-post 连接。http-post 填写上报地址和事件签名密钥，messageFormat 选择 array；关闭自身消息和离线消息上报。'
	}
} as const;
export type QqProvider = keyof typeof qqProviders;

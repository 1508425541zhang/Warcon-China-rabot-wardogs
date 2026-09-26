import { describe, expect, test } from 'bun:test';
import { hasTestDb, testEnv } from './db';
import { seedWorld } from './world';
import { callApi, callLoad } from './call';
import { aiSettings, saveAiSettings, aiBundle } from '$lib/server/integrity/ai';
import { decryptSecret } from '$lib/server/crypto';
describe.skipIf(!hasTestDb)('AI settings isolation', () => {
	test('encrypted key survives blank edit, is never loaded, destination change requires new key', async () => {
		const env = await testEnv();
		const w = await seedWorld(env);
		const input = {
			baseUrl: 'https://example.com/v1',
			model: 'test-model',
			apiKey: 'test-only-secret',
			maxTokens: 1200,
			tokenParameter: 'max_tokens'
		};
		await saveAiSettings(env, w.org.id, input);
		const saved = await aiSettings(env, w.org.id);
		expect(saved!.keyEnc).not.toContain(input.apiKey);
		expect(decryptSecret(env, saved!.keyEnc)).toBe(input.apiKey);
		await saveAiSettings(env, w.org.id, { ...input, apiKey: '' });
		expect((await aiSettings(env, w.org.id))!.keyEnc).toBe(saved!.keyEnc);
		await expect(
			saveAiSettings(env, w.org.id, { ...input, apiKey: '', baseUrl: 'https://other.example/v1' })
		).rejects.toThrow();
		const { load } = await import('../routes/(app)/server/[id]/integrity/ai/+page.server');
		const data = await callLoad(load, w.users.owner, { params: { id: w.server.id } });
		expect(data.status).toBe(200);
		expect(JSON.stringify(data)).not.toContain(input.apiKey);
		expect(JSON.stringify(data)).not.toContain(saved!.keyEnc);
		const { POST } = await import('../routes/api/servers/[id]/integrity/ai/+server');
		expect(
			(
				await callApi(POST, w.users.operator, {
					method: 'POST',
					params: { id: w.server.id },
					body: { operation: 'save', settings: input }
				})
			).status
		).toBe(403);
		await expect(aiBundle(env, w.org.id, w.server.id, 'other-case')).rejects.toThrow();
	});
});

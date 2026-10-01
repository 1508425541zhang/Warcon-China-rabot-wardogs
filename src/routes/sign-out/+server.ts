import { nativeJson } from '$lib/native/transport.server';
import { redirect, type RequestEvent } from '@sveltejs/kit';
export const POST = async (event: RequestEvent) => {
	await nativeJson(event, '/api/identity/logout', {});
	redirect(303, '/sign-in');
};
export const GET = () => redirect(303, '/');

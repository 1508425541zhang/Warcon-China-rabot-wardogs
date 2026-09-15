// A tiny Better Auth plugin for the sign-ins Better Auth has no provider for: Steam (OpenID 2.0)
// and recovery keys. The panel proves the identity itself, then asks this endpoint for a session,
// which keeps session rows, cookies, hooks and the login audit exactly as for every other method.
// Server-only: it is in disabledPaths and hooks.server.ts closes /api/auth/* anyway.
import * as z from 'zod';
import { APIError, createAuthEndpoint } from 'better-auth/api';
import { setSessionCookie } from 'better-auth/cookies';
import type { BetterAuthPlugin } from 'better-auth';

export const warconSessions = () =>
	({
		id: 'warcon-sessions',
		endpoints: {
			signInUser: createAuthEndpoint(
				'/warcon/sign-in-user',
				{ method: 'POST', body: z.object({ userId: z.string() }) },
				async (ctx) => {
					const user = await ctx.context.internalAdapter.findUserById(ctx.body.userId);
					if (!user)
						throw APIError.from('NOT_FOUND', {
							message: 'User not found.',
							code: 'USER_NOT_FOUND'
						});
					if ((user as { banned?: boolean | null }).banned)
						throw APIError.from('FORBIDDEN', {
							message: 'This account is disabled.',
							code: 'BANNED_USER'
						});
					const session = await ctx.context.internalAdapter.createSession(user.id);
					await setSessionCookie(ctx, { session, user });
					return ctx.json({ token: session.token, userId: user.id });
				}
			)
		}
	}) satisfies BetterAuthPlugin;

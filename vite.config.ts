import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Everything is rendered on the server: every page depends on the session and the database.
			adapter: adapter({ out: 'build' }),
			// Scripts are the panel's own (a nonce per response) and Turnstile's on the sign-up form.
			// Images are any https host: Steam and Discord avatars, and a server image is whatever
			// address its owner pasted. Styles allow inline because Svelte writes style attributes.
			// No form-action: signing in with Steam or Discord is a form post that redirects there.
			csp: {
				mode: 'nonce',
				directives: {
					'default-src': ['self'],
					'script-src': ['self', 'https://challenges.cloudflare.com'],
					'style-src': ['self', 'unsafe-inline'],
					'img-src': ['self', 'data:', 'https:'],
					'font-src': ['self', 'data:'],
					'connect-src': ['self'],
					'frame-src': ['https://challenges.cloudflare.com'],
					'frame-ancestors': ['none'],
					'base-uri': ['self'],
					'object-src': ['none']
				}
			}
		})
	],
	// Bun built-ins: resolved by the Bun runtime, never bundled.
	ssr: { external: ['bun'] },
	build: {
		cssMinify: 'lightningcss',
		rollupOptions: { external: ['bun'] }
	}
});

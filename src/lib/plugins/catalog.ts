import example from './extensions/round-summary/manifest.json';
import { pluginManifestSchema } from './sdk';

/** Add reviewed, compiled plugins here and register their component in components.ts. */
export const codePluginCatalog = [pluginManifestSchema.parse(example)];
export const knownPluginRenderer = (renderer: string) =>
	renderer === 'cards' || codePluginCatalog.some((p) => p.renderer === renderer);

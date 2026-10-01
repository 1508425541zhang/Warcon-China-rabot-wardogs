// Build-time metadata only. Runtime authorization and plugin installation are native Rust.
import { knownCauses } from '../../src/lib/causes';
import { codePluginCatalog } from '../../src/lib/plugins/catalog';
await Bun.write(
	new URL('../assets/causes.json', import.meta.url),
	JSON.stringify(knownCauses(), null, 2) + '\n'
);
await Bun.write(
	new URL('../assets/code-plugins.json', import.meta.url),
	JSON.stringify(codePluginCatalog, null, 2) + '\n'
);

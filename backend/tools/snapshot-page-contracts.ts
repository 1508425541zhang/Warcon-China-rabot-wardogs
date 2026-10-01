/** One-time DTO snapshot before replacing server pages; no runtime business is emitted. */
import ts from 'typescript';
import { readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, relative, join } from 'node:path';
const root = resolve(import.meta.dir, '../..');
function files(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory()
			? files(join(dir, e.name))
			: /^\+(page|layout)\.server\.ts$/.test(e.name)
				? [join(dir, e.name)]
				: []
	);
}
const config = ts.readConfigFile(join(root, 'tsconfig.json'), ts.sys.readFile);
const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
const routes = files(join(root, 'src/routes')).sort();
const program = ts.createProgram([...new Set([...parsed.fileNames, ...routes])], parsed.options);
const checker = program.getTypeChecker(),
	printer = ts.createPrinter();
let output =
	'// Native page data contracts. Generated from pre-migration page return types.\nexport interface PageContracts {\n';
const inventory: { source: string; actions: string[] }[] = [];
for (const path of routes) {
	const source = relative(root, path).replaceAll('\\', '/');
	const sf = program.getSourceFile(path)!;
	let type = '{}';
	let actions: string[] = [];
	for (const st of sf.statements) {
		if (!ts.isVariableStatement(st)) continue;
		for (const decl of st.declarationList.declarations) {
			if (!ts.isIdentifier(decl.name) || !decl.initializer) continue;
			if (decl.name.text === 'load') {
				const sig = checker.getTypeAtLocation(decl.initializer).getCallSignatures()[0];
				if (!sig) throw new Error(source);
				const ret = checker.getAwaitedType(checker.getReturnTypeOfSignature(sig))!;
				const node = checker.typeToTypeNode(
					ret,
					undefined,
					ts.NodeBuilderFlags.NoTruncation |
						ts.NodeBuilderFlags.UseStructuralFallback |
						ts.NodeBuilderFlags.UseFullyQualifiedType
				);
				if (!node) throw new Error('Type missing: ' + source);
				type = printer
					.printNode(ts.EmitHint.Unspecified, node, sf)
					.replaceAll(root.replaceAll('\\', '/') + '/src/lib/', '$lib/');
			}
			if (decl.name.text === 'actions' && ts.isObjectLiteralExpression(decl.initializer)) {
				actions = decl.initializer.properties
					.map((p) => p.name?.getText(sf).replace(/^['"]|['"]$/g, '') ?? '')
					.filter(Boolean);
			}
		}
	}
	inventory.push({ source, actions });
	output += JSON.stringify(source) + ': ' + type + ';\n';
}
output += '}\n';
mkdirSync(join(root, 'src/lib/native'), { recursive: true });
writeFileSync(join(root, 'src/lib/native/page-contracts.d.ts'), output);
writeFileSync(
	join(root, 'backend/assets/page-inventory.json'),
	JSON.stringify(inventory, null, 2) + '\n'
);
console.log('Saved ' + routes.length + ' contracts');

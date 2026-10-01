/** Top-level contract fields from the original DTO snapshot, independently of the native handlers. */
import ts from 'typescript';
import { readFileSync, writeFileSync } from 'node:fs';
const sf = ts.createSourceFile(
	'contracts.d.ts',
	readFileSync('src/lib/native/page-contracts.d.ts', 'utf8'),
	ts.ScriptTarget.Latest,
	true
);
const schema: Record<string, string[]> = {};
const contracts = sf.statements.find(
	(s) => ts.isInterfaceDeclaration(s) && s.name.text === 'PageContracts'
) as ts.InterfaceDeclaration;
for (const member of contracts.members) {
	if (!ts.isPropertySignature(member) || !member.type || !member.name) continue;
	const name = (member.name as ts.StringLiteral).text;
	const types = ts.isUnionTypeNode(member.type) ? member.type.types : [member.type];
	const keys = types.filter(ts.isTypeLiteralNode).map((t) =>
		t.members
			.filter(ts.isPropertySignature)
			.filter((p) => !p.questionToken)
			.map((p) => p.name.getText(sf).replace(/^['"]|['"]$/g, ''))
	);
	schema[name] = keys.length ? keys[0].filter((k) => keys.every((keys) => keys.includes(k))) : [];
}
writeFileSync('backend/assets/page-dto-fields.json', JSON.stringify(schema, null, 2) + '\n');

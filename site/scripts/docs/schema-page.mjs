// Renders docs/schema/document-v1.schema.json as a reference page. Everything below the
// introduction comes from the schema itself, so the page cannot drift from the file.

const REF = '#/$defs/';

/** Rustdoc text (intra-doc links, line breaks) as one Markdown line. */
function prose(text = '') {
	return text
		.replace(/\[(`[^`]+`)\](?!\()/g, '$1')
		.replace(/\[([^\]]+)\]\((?!https?:)[^)]*\)/g, '$1')
		.replace(/\s*\n\s*/g, ' ')
		.trim();
}

const cell = (s) => s.replace(/\|/g, '\\|');
const anchor = (name) => name.toLowerCase();

/** Short type expression for a schema node, with links to definitions on this page. */
export function typeOf(node) {
	if (!node || typeof node !== 'object') return 'any';
	if (node.$ref?.startsWith(REF)) {
		const name = node.$ref.slice(REF.length);
		return `[\`${name}\`](#${anchor(name)})`;
	}
	if ('const' in node) return `\`${JSON.stringify(node.const)}\``;
	if (node.enum) return node.enum.map((v) => `\`${JSON.stringify(v)}\``).join(', ');
	const union = node.oneOf ?? node.anyOf;
	if (union) return [...new Set(union.map(typeOf))].join(' or ');
	const types = Array.isArray(node.type) ? node.type : node.type ? [node.type] : [];
	const parts = types.map((t) => {
		if (t === 'array') return `array of ${typeOf(node.items)}`;
		if (t === 'object' && node.additionalProperties && typeof node.additionalProperties === 'object')
			return `map of ${typeOf(node.additionalProperties)}`;
		if ((t === 'integer' || t === 'number') && node.format) return `${t} (${node.format})`;
		return t;
	});
	return parts.length ? parts.join(' or ') : 'any';
}

function fieldTable(node) {
	const props = node.properties ?? {};
	const required = new Set(node.required ?? []);
	const names = Object.keys(props).filter((n) => !(n === 'type' && 'const' in (props[n] ?? {})));
	if (!names.length) return '';
	const rows = names.map((n) => {
		const p = props[n];
		return `| \`${n}\` | ${cell(typeOf(p))} | ${required.has(n) ? 'yes' : 'no'} | ${cell(prose(p.description))} |`;
	});
	return ['| Field | Type | Required | Description |', '|---|---|---|---|', ...rows].join('\n');
}

/** A tagged union: every variant is an object whose "type" property is a constant. */
function variants(node) {
	const union = node.oneOf ?? node.anyOf;
	if (!union) return undefined;
	const tagged = union.every((v) => v?.properties?.type && 'const' in v.properties.type);
	return tagged ? union : undefined;
}

function definition(name, node) {
	const out = [`### ${name}`, '', prose(node.description)];
	const tagged = variants(node);
	if (tagged) {
		out.push('', `Tagged by \`type\`. Variants:`, '');
		for (const v of tagged) {
			const tag = v.properties.type.const;
			out.push(`#### \`${tag}\``, '', prose(v.description));
			const table = fieldTable(v);
			if (table) out.push('', table);
			out.push('');
		}
		return out.join('\n');
	}
	if (node.properties) out.push('', fieldTable(node));
	else out.push('', `Type: ${typeOf(node)}.`);
	const union = node.oneOf ?? node.anyOf;
	if (union && union.some((u) => u.description)) {
		out.push('', ...union.filter((u) => u.description).map((u) => `- ${typeOf(u)}: ${prose(u.description)}`));
	}
	return out.join('\n');
}

/**
 * @param {object} schema  parsed JSON schema
 * @param {object} links   {raw, source, python, javascript, capi, architecture, changelog}
 */
export function schemaPage(schema, links) {
	const defs = schema.$defs ?? {};
	return [
		`Every cadkit interface can export a drawing as JSON in one shape: the format-neutral
\`Document\` model of \`cadkit-core\`. This page lists that shape, field by field, as
version 1 of the layout. It is generated from the committed JSON Schema
([\`document-v1.schema.json\`](${links.raw}), draft 2020-12), which \`cadkit-core\` derives from
the Rust types with its optional \`schemars\` feature.`,
		'',
		'## Versioning',
		'',
		`Exported JSON carries \`schema_version: 1\`; JSON without the field reads as version 1. The
version increases only when an existing field changes meaning or shape. New optional fields
do not change it, so consumers should ignore fields they do not know. The Rust importer
(\`document_from_json\`, which the bindings call) rejects other versions. See the [architecture notes](${links.architecture}) for coordinate
and unit conventions and the [0.5 migration notes](${links.changelog}) for model changes.`,
		'',
		'## Getting the JSON',
		'',
		'| Interface | Export | Import |',
		'|---|---|---|',
		'| Rust | `cadkit::to_json(&doc, pretty)` | `cadkit::export::document_from_json(text, &limits)` |',
		`| Python | \`doc.to_json(pretty=True)\`, \`doc.to_dict()\` | \`cadkit.Document.from_json(text)\` ([reference](${links.python})) |`,
		`| JavaScript | \`doc.toJsonString(true)\`, \`doc.toJSON()\` | \`CadDocument.fromJsonString(text)\` ([reference](${links.javascript})) |`,
		`| C | \`cadkit_document_to_json(doc, 1, &json)\` | \`cadkit_document_from_json(json, &doc)\` ([reference](${links.capi})) |`,
		'| CLI | `cadkit convert plan.dxf plan.json --pretty`, `cadkit dump --json plan.dxf` | |',
		'',
		`The raw schema is served at [\`${links.raw}\`](${links.raw}) for validators and editors;
its source is [\`${links.sourcePath}\`](${links.source}).`,
		'',
		'## Document',
		'',
		prose(schema.description),
		'',
		fieldTable(schema),
		'',
		'## Definitions',
		'',
		...Object.keys(defs)
			.sort()
			.flatMap((name) => [definition(name, defs[name]), '']),
	].join('\n');
}

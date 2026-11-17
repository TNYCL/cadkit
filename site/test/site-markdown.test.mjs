import { test } from 'node:test';
import assert from 'node:assert/strict';
import { htmlPageMarkdown, markdownPathFor } from '../src/lib/site-markdown.mjs';

test('Markdown routes include the home page', () => {
	assert.equal(markdownPathFor('/'), '/index.md');
	assert.equal(markdownPathFor('/formats/dwg/'), '/formats/dwg.md');
	assert.equal(markdownPathFor('/docs/getting-started/rust/'), '/docs/getting-started/rust.md');
});

const meta = { title: 'Format reference', description: 'Read and write formats.', url: 'https://cadkit.org/formats/' };

test('HTML export keeps all code tabs, exact code lines, tables and absolute links', () => {
	const md = htmlPageMarkdown(`<header>Site navigation</header><main>
	<h1>Format reference</h1><p>Read <a href="./dwg/#support">DWG</a> &amp; DXF.</p>
	<div role="tablist"><a id="rust" role="tab">Rust</a><a id="js" role="tab">JavaScript</a></div>
	<div role="tabpanel" aria-labelledby="rust"><pre data-language="rust"><code><div class="ec-line"><div>fn main() {</div></div><div class="ec-line"><div>    let x = &quot;&#96;&#96;&#96;&quot;;</div></div><div class="ec-line"><div>}</div></div></code></pre></div>
	<div role="tabpanel" aria-labelledby="js" hidden><pre data-language="js"><code>const x = 1;</code></pre></div>
	<table><thead><tr><th>Format</th><th>Read</th></tr></thead><tbody><tr><td>DWG</td><td><span>R14</span><span>to 2018</span></td></tr></tbody></table>
	<p><a href="mailto:hello@cadkit.org">Contact</a></p></main><footer>Site footer</footer>`, meta);
	assert.match(md, /Canonical page: https:\/\/cadkit.org\/formats\//);
	assert.match(md, /\[DWG\]\(https:\/\/cadkit.org\/formats\/dwg\/#support\)/);
	assert.match(md, /\*\*Rust\*\*/);
	assert.match(md, /````rust\nfn main\(\) \{\n    let x = "```";\n\}/);
	assert.match(md, /\*\*JavaScript\*\*[\s\S]*```js\nconst x = 1;/);
	assert.match(md, /\| Format \| Read \|\n\| --- \| --- \|\n\| DWG \| R14 to 2018 \|/);
	assert.match(md, /\[Contact\]\(mailto:hello@cadkit.org\)/);
	assert.doesNotMatch(md, /Site navigation|Site footer/);
});

test('HTML export omits decorative and interactive content but keeps disclosure data', () => {
	const md = htmlPageMarkdown(`<main><h1>Format reference</h1>
	<p>Text with &lt;tag&gt; and &#x1f600;.</p><svg><title>Chart dots</title></svg>
	<div aria-hidden="true">Decoration</div><div hidden>Working on your drawing</div><button>Copy code</button><script>secret()</script>
	<details><summary>Data</summary><p>42 downloads</p></details>
	<ul><li>First<ul><li>Nested</li></ul></li><li>Second</li></ul>
	<p><code>a&#96;b</code> <a href="javascript:alert(1)">Unsafe link</a></p></main>`, meta);
	assert.match(md, /Text with \\<tag\\> and 😀/);
	assert.match(md, /42 downloads/);
	assert.match(md, /- First\n\n  - Nested/);
	assert.match(md, /``a`b``/);
	assert.doesNotMatch(md, /Chart dots|Decoration|Working on your drawing|Copy code|secret|javascript:/);
	assert.throws(() => htmlPageMarkdown('<body>Missing content</body>', meta), /missing <main>/);
});

test('script strings cannot swallow later tabs or sections of a rendered page', () => {
	const md = htmlPageMarkdown(`<main><h1>Format reference</h1>
	<figure><pre data-language="rust"><code>fn main() {}</code></pre>
	<button>Copy<script>const icon = '<dyn><svg><path /></svg></dyn>';</script></button></figure>
	<pre data-language="python"><code>print(42)</code></pre>
	<section><h2>Next section</h2><p>All content remains.</p></section></main>`, meta);
	assert.match(md, /```rust\nfn main\(\) \{\}/);
	assert.match(md, /```python\nprint\(42\)/);
	assert.match(md, /## Next section\n\nAll content remains\./);
	assert.doesNotMatch(md, /const icon|dyn|Copy/);
});

test('source line breaks survive Expressive Code blank-line sentinels and Markdown spacing', () => {
	const md = htmlPageMarkdown('<main><h1>Title</h1><pre data-language="python"><code>' +
		'<div class="ec-line"><div>first = 1</div></div>' +
		'<div class="ec-line"><div>\n</div></div><div class="ec-line"><div>\n</div></div>' +
		'<div class="ec-line"><div>second = 2</div></div></code></pre></main>', meta);
	assert.match(md, /```python\nfirst = 1\n\n\nsecond = 2\n```/);
});

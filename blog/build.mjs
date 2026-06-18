import { promises as fs, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const srcDir = path.join(here, "src");
const htmlDir = path.join(here, "html");
const distDir = path.join(here, "dist");
const basePath = normalizeBasePath(process.env.BLOG_BASE_PATH ?? "/blog/");
const outputRoot = path.join(distDir, basePath.replace(/^\/+|\/+$/g, ""));
const siteName = "Uldren Engineering Blog";

const markdownFiles = (await listFiles(srcDir)).filter((file) => file.endsWith(".md"));
const documents = markdownFiles.map((file) => loadMarkdown(file));
const slugBySource = new Map(documents.map((doc) => [doc.sourceName, doc.slug]));
const pages = documents
  .filter((doc) => doc.status === "ready")
  .sort((left, right) => right.date.localeCompare(left.date) || left.title.localeCompare(right.title));

await resetDir(distDir);
await copyDir(htmlDir, outputRoot);
await writeIndex(pages);

for (const page of pages) {
  await writePost(page);
}

console.log(`blog: wrote ${pages.length} posts to ${relative(outputRoot)}`);

async function writeIndex(posts) {
  const items = posts
    .map((post) => {
      const href = postUrl(post);
      return `<li class="post-list-item">
  <a href="${escapeAttr(href)}">${escapeHtml(post.title)}</a>
  <time datetime="${escapeAttr(post.date)}">${escapeHtml(formatDate(post.date))}</time>
  <p>${escapeHtml(post.summary)}</p>
</li>`;
    })
    .join("\n");

  const content = `<section class="index-head">
  <h1>Uldren Engineering Blog</h1>
  <p>Technical notes on agent workflows, applied AI systems, and durable software interfaces.</p>
</section>
<ol class="post-list">
${items}
</ol>`;

  await fs.writeFile(
    path.join(outputRoot, "index.html"),
    layout({
      title: `${siteName}`,
      canonicalPath: basePath,
      description: "Engineering notes from Uldren.",
      bodyClass: "index",
      content,
    }),
  );
}

async function writePost(post) {
  const outputDir = path.join(outputRoot, post.slug);
  await fs.mkdir(outputDir, { recursive: true });

  const rendered = renderMarkdown(post.body, { skipFirstTitle: post.title });
  const content = `<article class="markdown-page">
  <header class="article-head">
    <h1>${escapeHtml(post.title)}</h1>
    <div class="article-meta">
      <time datetime="${escapeAttr(post.date)}">${escapeHtml(formatDate(post.date))}</time>
      <span>${escapeHtml(post.author)}</span>
    </div>
    <p>${escapeHtml(post.summary)}</p>
  </header>
  <div class="markdown-body">
${rendered}
  </div>
</article>`;

  await fs.writeFile(
    path.join(outputDir, "index.html"),
    layout({
      title: `${post.title} | ${siteName}`,
      canonicalPath: postUrl(post),
      description: post.summary,
      bodyClass: "post",
      content,
    }),
  );
}

function layout({ title, canonicalPath, description, bodyClass, content }) {
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escapeHtml(title)}</title>
  <meta name="description" content="${escapeAttr(description)}">
  <link rel="canonical" href="${escapeAttr(canonicalPath)}">
  <link rel="stylesheet" href="${escapeAttr(assetUrl("assets/blog.css"))}">
  <script defer src="https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js"></script>
  <script defer src="${escapeAttr(assetUrl("assets/blog.js"))}"></script>
  <script type="module">
    import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";
    mermaid.initialize({
      startOnLoad: true,
      theme: "neutral",
      securityLevel: "strict",
      flowchart: {
        htmlLabels: true,
        wrappingWidth: 180,
      },
    });
  </script>
</head>
<body class="${escapeAttr(bodyClass)}">
  <header class="site-bar">
    <a class="site-mark" href="${escapeAttr(basePath)}">Uldren</a>
    <nav aria-label="Primary">
      <a href="${escapeAttr(basePath)}">Blog</a>
      <a href="/">Home</a>
    </nav>
  </header>
  <main>
${content}
  </main>
  <footer class="site-footer">
    <a href="${escapeAttr(basePath)}">Index</a>
    <span>Uldren Engineering</span>
  </footer>
</body>
</html>
`;
}

function loadMarkdown(file) {
  const text = fsSyncRead(file);
  const { data, body } = parseFrontMatter(text);
  const sourceName = path.basename(file);
  const fallbackSlug = sourceName.replace(/\.md$/, "");
  const title = data.title ?? titleFromSlug(fallbackSlug);
  const slug = slugify(data.slug ?? fallbackSlug);

  return {
    sourceName,
    title,
    slug,
    date: data.date ?? "",
    author: data.author ?? "Uldren",
    summary: data.summary ?? "",
    status: data.status ?? "draft",
    body,
  };
}

function parseFrontMatter(text) {
  if (!text.startsWith("---\n")) {
    return { data: {}, body: text };
  }

  const end = text.indexOf("\n---", 4);
  if (end === -1) {
    return { data: {}, body: text };
  }

  const frontMatter = text.slice(4, end).trim();
  const body = text.slice(end + 4).replace(/^\n/, "");
  const data = {};

  for (const line of frontMatter.split("\n")) {
    const match = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
    if (!match) {
      continue;
    }

    data[match[1]] = stripQuotes(match[2].trim());
  }

  return { data, body };
}

function renderMarkdown(markdown, options = {}) {
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  const html = [];
  let paragraph = [];
  let list = null;
  let blockquote = [];
  let code = null;
  let skippedFirstTitle = false;

  const closeParagraph = () => {
    if (paragraph.length === 0) {
      return;
    }
    html.push(`<p>${renderInline(paragraph.join(" "))}</p>`);
    paragraph = [];
  };

  const closeList = () => {
    if (!list) {
      return;
    }
    html.push(`</${list}>`);
    list = null;
  };

  const closeBlockquote = () => {
    if (blockquote.length === 0) {
      return;
    }
    html.push(`<blockquote>${blockquote.map((line) => `<p>${renderInline(line)}</p>`).join("")}</blockquote>`);
    blockquote = [];
  };

  for (const line of lines) {
    const fence = line.match(/^```([A-Za-z0-9_-]*)\s*$/);
    if (fence) {
      closeParagraph();
      closeList();
      closeBlockquote();

      if (code) {
        html.push(renderFence(code.lang, code.lines.join("\n")));
        code = null;
      } else {
        code = { lang: fence[1].toLowerCase(), lines: [] };
      }
      continue;
    }

    if (code) {
      code.lines.push(line);
      continue;
    }

    if (line.trim() === "") {
      closeParagraph();
      closeList();
      closeBlockquote();
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      closeParagraph();
      closeList();
      closeBlockquote();
      const level = heading[1].length;
      const text = heading[2].trim();
      if (!skippedFirstTitle && level === 1 && options.skipFirstTitle === text) {
        skippedFirstTitle = true;
        continue;
      }
      html.push(`<h${level} id="${escapeAttr(slugify(text))}">${renderInline(text)}</h${level}>`);
      continue;
    }

    const unordered = line.match(/^\s*-\s+(.+)$/);
    const ordered = line.match(/^\s*\d+\.\s+(.+)$/);
    if (unordered || ordered) {
      closeParagraph();
      closeBlockquote();
      const tag = unordered ? "ul" : "ol";
      if (list !== tag) {
        closeList();
        html.push(`<${tag}>`);
        list = tag;
      }
      html.push(`<li>${renderInline((unordered ?? ordered)[1])}</li>`);
      continue;
    }

    const quote = line.match(/^>\s?(.*)$/);
    if (quote) {
      closeParagraph();
      closeList();
      blockquote.push(quote[1]);
      continue;
    }

    closeList();
    closeBlockquote();
    paragraph.push(line.trim());
  }

  closeParagraph();
  closeList();
  closeBlockquote();

  if (code) {
    html.push(renderFence(code.lang, code.lines.join("\n")));
  }

  return html.join("\n");
}

function renderFence(lang, raw) {
  if (lang === "mermaid") {
    return `<pre class="mermaid">${escapeHtml(raw)}</pre>`;
  }

  if (lang === "chart" || lang === "chartjs") {
    return `<figure class="chart-figure">
  <canvas></canvas>
  <script type="application/json">${escapeHtml(raw)}</script>
</figure>`;
  }

  const className = lang ? ` class="language-${escapeAttr(lang)}"` : "";
  return `<pre><code${className}>${escapeHtml(raw)}</code></pre>`;
}

function renderInline(text) {
  const codeSpans = [];
  const tokenized = text.replace(/`([^`]+)`/g, (_, codeText) => {
    const token = `\u0000${codeSpans.length}\u0000`;
    codeSpans.push(`<code>${escapeHtml(codeText)}</code>`);
    return token;
  });

  const escaped = escapeHtml(tokenized).replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, label, href) => {
    return `<a href="${escapeAttr(rewriteHref(href))}">${label}</a>`;
  });

  return escaped.replace(/\u0000(\d+)\u0000/g, (_, index) => codeSpans[Number(index)]);
}

function rewriteHref(href) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("#") || href.startsWith("/")) {
    return href;
  }

  const [target, anchor = ""] = href.split("#");
  if (!target.endsWith(".md")) {
    return href;
  }

  const sourceName = path.basename(target);
  const slug = slugBySource.get(sourceName) ?? sourceName.replace(/\.md$/, "");
  return `${basePath}${slug}/${anchor ? `#${anchor}` : ""}`;
}

async function listFiles(dir) {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await listFiles(fullPath)));
    } else {
      files.push(fullPath);
    }
  }

  return files;
}

async function resetDir(dir) {
  await fs.rm(dir, { recursive: true, force: true });
  await fs.mkdir(dir, { recursive: true });
}

async function copyDir(from, to) {
  try {
    await fs.access(from);
  } catch {
    return;
  }

  const entries = await fs.readdir(from, { withFileTypes: true });
  await fs.mkdir(to, { recursive: true });

  for (const entry of entries) {
    const source = path.join(from, entry.name);
    const target = path.join(to, entry.name);
    if (entry.isDirectory()) {
      await copyDir(source, target);
    } else {
      await fs.copyFile(source, target);
    }
  }
}

function assetUrl(assetPath) {
  return `${basePath}${assetPath}`;
}

function postUrl(post) {
  return `${basePath}${post.slug}/`;
}

function normalizeBasePath(input) {
  const withLeading = input.startsWith("/") ? input : `/${input}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

function formatDate(value) {
  if (!value) {
    return "";
  }
  const date = new Date(`${value}T00:00:00Z`);
  return new Intl.DateTimeFormat("en", {
    month: "long",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  }).format(date);
}

function titleFromSlug(slug) {
  return slug
    .split("-")
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function slugify(value) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function stripQuotes(value) {
  if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
    return value.slice(1, -1);
  }
  return value;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function escapeAttr(value) {
  return escapeHtml(value);
}

function relative(file) {
  return path.relative(process.cwd(), file) || ".";
}

function fsSyncRead(file) {
  return readFileSync(file, "utf8");
}

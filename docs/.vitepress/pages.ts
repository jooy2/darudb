/**
 * Reading a page's own summary out of its Markdown source.
 *
 * Every page opens with one sentence that says what it is about. That sentence
 * is its `<meta name="description">`, its line in `llms.txt`, and its line in
 * the lists of the API and Types overviews, so it is read the same way for
 * all three: the config reads it from the file, and the data loader behind the
 * overviews from the source VitePress hands it.
 */

/** Inline Markdown and HTML dropped: a summary carries text and nothing else. */
export function plainText(source: string): string {
  return source
    .replace(/<[^>]+>/g, ' ')
    .replace(/!\[[^\]]*]\([^)]*\)/g, ' ')
    .replace(/\[([^\]]*)]\([^)]*\)/g, '$1')
    .replace(/[`*_]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Cut at a word boundary, to about what a result page will show whole. */
export function clamp(text: string, limit = 160): string {
  if (text.length <= limit) {
    return text;
  }

  const cut = text.slice(0, limit);
  const boundary = cut.lastIndexOf(' ');

  return `${(boundary > 0 ? cut.slice(0, boundary) : cut).trimEnd()}…`;
}

/**
 * A page's own one-line summary: the first block that is prose rather than the
 * title, a fenced example, a table or a container.
 */
export function summaryFromSource(source: string, limit = 160): string | undefined {
  for (const block of source.replace(/^---\r?\n[\s\S]*?\r?\n---/, '').split(/\n\s*\n/)) {
    const trimmed = block.trim();

    // A heading, HTML, a fence, a container, a table, a quote or a list. A
    // paragraph that opens with inline code, as a reference page's
    // "`Database` is …" does, is prose.
    if (!trimmed || /^(?:[#<:|>-]|```)/.test(trimmed)) {
      continue;
    }

    const text = plainText(trimmed);

    if (text) {
      return clamp(text, limit);
    }
  }

  return undefined;
}

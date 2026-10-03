// Vite's own declarations for what the theme imports that is not TypeScript:
// stylesheets imported for their side effect, and assets.
/// <reference types="vite/client" />

// Single-file components, which `tsc` cannot read. VitePress compiles them;
// the type checker only needs to know that each one is a component.
declare module '*.vue' {
  import type { DefineComponent } from 'vue';

  const component: DefineComponent;
  export default component;
}

// The container plugin ships no types of its own, and the community package's
// are written against another release of the parser's types than the one
// VitePress bundles. It is a plugin of VitePress's parser that takes a name
// and options; that is all the config needs to know.
declare module 'markdown-it-container' {
  import type { MarkdownRenderer } from 'vitepress';

  const container: (md: MarkdownRenderer, name: string, options?: object) => void;
  export default container;
}

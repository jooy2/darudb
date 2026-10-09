import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import LangCode from './LangCode.vue';
import Layout from './Layout.vue';
import PageList from './PageList.vue';
import './brand.css';
import './home.css';
import './languages.css';
import './packages.css';

export default {
  extends: DefaultTheme,
  // The default layout with the language switch above the sidebar and the
  // package menu in the navbar.
  Layout,
  enhanceApp({ app }) {
    app.component('LangCode', LangCode);
    app.component('PageList', PageList);
  }
} satisfies Theme;

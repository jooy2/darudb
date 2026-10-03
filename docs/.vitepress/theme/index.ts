import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import LangCode from './LangCode.vue';
import Layout from './Layout.vue';
import PageList from './PageList.vue';
import './brand.css';
import './home.css';
import './languages.css';

export default {
  extends: DefaultTheme,
  // The default layout with the language switch above the sidebar.
  Layout,
  enhanceApp({ app }) {
    app.component('LangCode', LangCode);
    app.component('PageList', PageList);
  }
} satisfies Theme;

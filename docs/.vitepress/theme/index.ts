import DefaultTheme from 'vitepress/theme';
import { inBrowser, type Theme } from 'vitepress';
import { forwardedPath } from '../languages';
import CompareTable from './CompareTable.vue';
import LangCode from './LangCode.vue';
import Layout from './Layout.vue';
import PageList from './PageList.vue';
import { chosenLanguage } from './language';
import './brand.css';
import './compare.css';
import './home.css';
import './languages.css';
import './packages.css';

export default {
  extends: DefaultTheme,
  // The default layout with the language switch above the sidebar and the
  // package menu in the navbar.
  Layout,
  enhanceApp({ app, router, siteData }) {
    app.component('CompareTable', CompareTable);
    app.component('LangCode', LangCode);
    app.component('PageList', PageList);

    if (!inBrowser) {
      return;
    }

    const localePrefixes = Object.keys(siteData.value.locales).filter((key) => key !== 'root');

    /*
     * A link to the overview of a forwarded section, such as the navbar's
     * Changelog, goes to the reader's page in that section instead, the way
     * the script in `<head>` sends a reader who loads the overview itself.
     *
     * The router also comes through here once as the app starts, for the page
     * the document already is. When that is an overview, the script in
     * `<head>` has begun replacing the document, and a history entry pushed
     * now could cancel that, so the same replacement is asked for again.
     */
    router.onBeforeRouteChange = (to) => {
      const target = forwardedPath(to, chosenLanguage(), localePrefixes);

      if (!target) {
        return;
      }

      if (forwardedPath(location.pathname, chosenLanguage(), localePrefixes)) {
        location.replace(target);
      } else {
        router.go(target);
      }

      return false;
    };
  }
} satisfies Theme;

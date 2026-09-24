import { globalIgnores } from 'eslint/config';
import pluginJs from '@eslint/js';
import pluginTypeScriptESLint from 'typescript-eslint';
import parserTypeScript from '@typescript-eslint/parser';
import pluginNode from 'eslint-plugin-n';
import configPrettier from 'eslint-config-prettier';

import globals from 'globals';

export default pluginTypeScriptESLint.config(
  pluginJs.configs.recommended,
  pluginTypeScriptESLint.configs.recommended,
  pluginNode.configs['flat/recommended-script'],
  globalIgnores([
    '**/.idea',
    '**/.vscode',
    '**/node_modules',
    // Written by `npm run build` from `src/lib.rs`. The Rust source is what is
    // reviewed; the loader napi-rs generates for it is not ours to lint.
    'native.js',
    'native.d.ts',
    // Written by `tsc` from `lib`.
    'dist',
    'npm',
    '**/*-lock.json',
    '**/*-lock.yaml'
  ]),
  {
    files: ['**/*.{js,mjs,cjs,ts}'],
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: {
        ...globals.node
      },
      parserOptions: {
        parser: parserTypeScript,
        ecmaVersion: 2022,
        requireConfigFile: false
      }
    },
    rules: {
      eqeqeq: 'error',
      'no-unused-vars': 'off',
      'no-case-declarations': 'off',
      'no-trailing-spaces': 'error',
      'no-unsafe-optional-chaining': 'off',
      'no-control-regex': 'off',
      'n/no-missing-import': 'off',
      'n/no-unpublished-import': 'off',
      'n/no-unsupported-features/node-builtins': 'off',
      '@typescript-eslint/no-explicit-any': 'off'
    }
  },
  {
    // `import x = require(...)` compiles to a plain `require`, where the other
    // forms would add getters: a namespace import of `native.js` would wrap it
    // in an object of getters, which every call into the engine would then go
    // through, and a name `index.ts` imported and exported again would be a
    // getter on the package's exports rather than a value.
    files: ['lib/**/*.ts'],
    rules: {
      '@typescript-eslint/no-require-imports': ['error', { allowAsImport: true }]
    }
  },
  configPrettier
);

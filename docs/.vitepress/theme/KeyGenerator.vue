<script setup lang="ts">
import { useData } from 'vitepress';
import { computed, nextTick, onBeforeUnmount, ref } from 'vue';
import { localeOf, t, type StringKey } from './i18n';

/*
 * The button on the encryption guide that makes a key: 32 random bytes, the
 * length `key` takes in every language, shown as hex and as Base64.
 *
 * The bytes come from `crypto.getRandomValues`, the browser's
 * cryptographically secure generator. They stay in this component: nothing
 * stores them or sends them anywhere, and leaving the page forgets them.
 * Nothing is made until the button is pressed, so the page the server renders
 * holds no key, and every reader's key is their own.
 *
 * The key itself is not in the live region, since a screen reader would read
 * all 64 characters every time; the region says that a key was made or copied,
 * and the key is in the list below the button for the reader to go to.
 */

const KEY_BYTES = 32;

/** How long a copy button says it copied before it says "Copy" again. */
const COPIED_MS = 2000;

type FormId = 'hex' | 'base64';

interface Form {
  id: FormId;
  label: StringKey;
  copyLabel: StringKey;
  text: string;
}

const { lang } = useData();
const locale = computed(() => localeOf(lang.value));
const key = ref<Uint8Array | null>(null);
const copied = ref<FormId | null>(null);
const status = ref<StringKey | null>(null);
let copiedTimer: ReturnType<typeof setTimeout> | undefined;

const forms = computed<Form[]>(() => {
  const bytes = key.value;

  if (!bytes) {
    return [];
  }

  return [
    {
      id: 'hex',
      label: 'keyHex',
      copyLabel: 'keyCopyHex',
      text: Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
    },
    {
      id: 'base64',
      label: 'keyBase64',
      copyLabel: 'keyCopyBase64',
      text: btoa(String.fromCharCode(...bytes))
    }
  ];
});

/**
 * Says `message` in the live region, emptying it first so that the same
 * message said twice, for a second key, is read twice.
 */
async function announce(message: StringKey) {
  status.value = null;
  await nextTick();
  status.value = message;
}

function make() {
  const bytes = new Uint8Array(KEY_BYTES);

  crypto.getRandomValues(bytes);
  key.value = bytes;
  copied.value = null;
  void announce('keyMade');
}

async function copy(form: Form) {
  clearTimeout(copiedTimer);

  try {
    await navigator.clipboard.writeText(form.text);
  } catch {
    // The clipboard needs a secure page and, in some browsers, a permission.
    // The key is still on the page, and a click selects it whole.
    copied.value = null;
    void announce('keyCopyFailed');

    return;
  }

  copied.value = form.id;
  void announce('keyCopiedStatus');
  copiedTimer = setTimeout(() => {
    copied.value = null;
  }, COPIED_MS);
}

onBeforeUnmount(() => clearTimeout(copiedTimer));
</script>

<template>
  <div class="key-generator">
    <div class="key-generator-head">
      <button type="button" class="key-generator-make" @click="make">
        {{ t(locale, key ? 'keyAgain' : 'keyMake') }}
      </button>
      <span class="key-generator-size">{{ t(locale, 'keySize') }}</span>
    </div>

    <dl v-if="key" class="key-generator-forms">
      <div v-for="form in forms" :key="form.id" class="key-generator-form">
        <dt>{{ t(locale, form.label) }}</dt>
        <dd>
          <code class="key-generator-value">{{ form.text }}</code>
          <button
            type="button"
            class="key-generator-copy"
            :aria-label="t(locale, form.copyLabel)"
            @click="copy(form)"
          >
            {{ t(locale, copied === form.id ? 'keyCopied' : 'keyCopy') }}
          </button>
        </dd>
      </div>
    </dl>

    <p
      class="key-generator-status"
      :class="{ 'visually-hidden': status !== 'keyCopyFailed' }"
      aria-live="polite"
    >
      {{ status ? t(locale, status) : '' }}
    </p>
  </div>
</template>

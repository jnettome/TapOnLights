<script lang="ts">
  let {
    checked,
    label,
    disabled = false,
    onchange,
  }: { checked: boolean; label: string; disabled?: boolean; onchange: (value: boolean) => void } = $props();
</script>

<button
  type="button"
  role="switch"
  class="toggle"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={(e) => {
    e.stopPropagation();
    onchange(!checked);
  }}><span></span></button>

<style>
  .toggle {
    position: relative;
    flex: none;
    width: 40px;
    height: 22px;
    border-radius: 11px;
    border: 1px solid var(--stroke-strong);
    background: var(--track);
    transition: background 0.2s var(--ease), border-color 0.2s var(--ease);
  }
  .toggle span {
    position: absolute;
    top: 50%;
    left: 4px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--text-2);
    transform: translateY(-50%);
    transition: transform 0.2s var(--ease), width 0.2s var(--ease), background 0.2s;
  }
  .toggle:active:not(:disabled) span {
    width: 15px;
  }
  .toggle[aria-checked="true"] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .toggle[aria-checked="true"] span {
    background: var(--accent-text);
    transform: translate(18px, -50%);
  }
  .toggle[aria-checked="true"]:active:not(:disabled) span {
    transform: translate(15px, -50%);
  }
  .toggle:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

<script lang="ts">
  // Pill slider. A transparent native range input sits on top for pointer,
  // touch and keyboard handling; the visuals are drawn underneath.
  let {
    value,
    min,
    max,
    step = 1,
    label,
    track,
    fill = null,
    disabled = false,
    oninput,
    onstart,
    onend,
  }: {
    value: number;
    min: number;
    max: number;
    step?: number;
    label: string;
    /** CSS background of the whole track. */
    track: string;
    /** CSS background of the filled part; null shows a knob instead of a fill. */
    fill?: string | null;
    disabled?: boolean;
    oninput: (value: number) => void;
    onstart?: () => void;
    onend?: () => void;
  } = $props();

  const pct = $derived(((Math.min(max, Math.max(min, value)) - min) / (max - min)) * 100);
</script>

<div class="slider" class:disabled style:--slider-track={track} style:--slider-fill={fill} style:--pct="{pct}%">
  {#if fill}
    <div class="fill"></div>
  {:else}
    <div class="knob"></div>
  {/if}
  <input
    type="range"
    aria-label={label}
    {min}
    {max}
    {step}
    {value}
    {disabled}
    oninput={(e) => oninput(Number(e.currentTarget.value))}
    onpointerdown={() => onstart?.()}
    onpointerup={() => onend?.()}
    onpointercancel={() => onend?.()}
    onkeydown={() => onstart?.()}
    onkeyup={() => onend?.()}
    onblur={() => onend?.()} />
</div>

<style>
  .slider {
    position: relative;
    height: 40px;
    border-radius: 12px;
    background: var(--slider-track);
    box-shadow: inset 0 0 0 1px var(--stroke-strong);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    width: max(var(--pct), 22px);
    background: var(--slider-fill);
    border-radius: 12px;
    transition: width 0.08s linear;
  }
  .fill::after {
    content: "";
    position: absolute;
    top: 50%;
    right: 8px;
    width: 4px;
    height: 16px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.85);
    transform: translateY(-50%);
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.06);
  }
  .knob {
    position: absolute;
    top: 50%;
    left: clamp(14px, var(--pct), calc(100% - 14px));
    width: 22px;
    height: 30px;
    border-radius: 8px;
    border: 3px solid #fff;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
    transform: translate(-50%, -50%);
    transition: left 0.08s linear;
  }
  input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
    touch-action: none;
  }
  input::-webkit-slider-thumb {
    appearance: none;
    width: 2px;
    height: 40px;
  }
  input::-moz-range-thumb {
    width: 2px;
    height: 40px;
    border: 0;
  }
  .slider:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .disabled {
    opacity: 0.45;
  }
  .disabled input {
    cursor: default;
  }
</style>

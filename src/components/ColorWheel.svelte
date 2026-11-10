<script lang="ts">
  import { hueSatCss } from "../lib/color";
  import type { HueSat } from "../lib/types";

  // Hue around the circle (0° at the top, clockwise), saturation from center to edge.
  let {
    hue,
    saturation,
    size = 176,
    disabled = false,
    oninput,
    onstart,
    onend,
  }: {
    hue: number;
    saturation: number;
    size?: number;
    disabled?: boolean;
    oninput: (value: HueSat) => void;
    onstart?: () => void;
    onend?: () => void;
  } = $props();

  let wheel: HTMLDivElement;
  let dragging = false;

  const thumb = $derived.by(() => {
    const r = (saturation / 100) * (size / 2 - 12);
    const a = (hue * Math.PI) / 180;
    return { x: size / 2 + r * Math.sin(a), y: size / 2 - r * Math.cos(a) };
  });

  function pick(e: PointerEvent) {
    const box = wheel.getBoundingClientRect();
    const dx = e.clientX - (box.left + box.width / 2);
    const dy = e.clientY - (box.top + box.height / 2);
    const radius = box.width / 2 - 12;
    const h = Math.round(((Math.atan2(dx, -dy) * 180) / Math.PI + 360) % 360);
    const s = Math.round(Math.min(1, Math.hypot(dx, dy) / radius) * 100);
    oninput({ hue: h, saturation: Math.max(1, s) });
  }

  function down(e: PointerEvent) {
    if (disabled) return;
    dragging = true;
    wheel.setPointerCapture(e.pointerId);
    onstart?.();
    pick(e);
  }

  function move(e: PointerEvent) {
    if (dragging) pick(e);
  }

  function up() {
    if (!dragging) return;
    dragging = false;
    onend?.();
  }

  function key(e: KeyboardEvent) {
    const steps: Record<string, [number, number]> = {
      ArrowLeft: [-5, 0],
      ArrowRight: [5, 0],
      ArrowUp: [0, 5],
      ArrowDown: [0, -5],
    };
    const step = steps[e.key];
    if (!step || disabled) return;
    e.preventDefault();
    onstart?.();
    oninput({
      hue: (hue + step[0] + 360) % 360,
      saturation: Math.min(100, Math.max(1, saturation + step[1])),
    });
    onend?.();
  }
</script>

<div
  class="wheel"
  class:disabled
  bind:this={wheel}
  style:width="{size}px"
  style:height="{size}px"
  role="slider"
  tabindex={disabled ? -1 : 0}
  aria-label="Color"
  aria-valuemin={0}
  aria-valuemax={360}
  aria-valuenow={hue}
  aria-valuetext="Hue {hue} degrees, saturation {saturation}%"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onkeydown={key}>
  <div class="thumb" style:left="{thumb.x}px" style:top="{thumb.y}px" style:background={hueSatCss({ hue, saturation })}></div>
</div>

<style>
  .wheel {
    position: relative;
    flex: none;
    border-radius: 50%;
    background:
      radial-gradient(closest-side, #fff 0%, rgba(255, 255, 255, 0.85) 18%, rgba(255, 255, 255, 0) 100%),
      conic-gradient(
        from 0deg,
        hsl(0 100% 50%),
        hsl(60 100% 50%),
        hsl(120 100% 50%),
        hsl(180 100% 50%),
        hsl(240 100% 50%),
        hsl(300 100% 50%),
        hsl(360 100% 50%)
      );
    box-shadow:
      inset 0 0 0 1px rgba(0, 0, 0, 0.08),
      0 4px 18px rgba(0, 0, 0, 0.12);
    cursor: crosshair;
    touch-action: none;
  }
  .thumb {
    position: absolute;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 3px solid #fff;
    box-shadow:
      0 0 0 1px rgba(0, 0, 0, 0.12),
      0 2px 8px rgba(0, 0, 0, 0.35);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }
  .disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>

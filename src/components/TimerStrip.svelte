<script lang="ts">
  import { errorText, type Api } from "../lib/api";
  import { formatRemaining, progress, timerTitle } from "../lib/timers";
  import type { Timer } from "../lib/types";
  import Icon from "./Icon.svelte";

  let {
    api,
    timers,
    now,
    onopen,
    ontoast,
  }: {
    api: Api;
    /** Ringing first, then by end time (as the snapshot sends them). */
    timers: Timer[];
    now: number;
    onopen: () => void;
    ontoast: (message: string) => void;
  } = $props();

  const t = $derived(timers[0]);
  const ringing = $derived(!!t?.ringingSinceMs);
  const more = $derived(timers.length - 1);

  function extend(m: number) {
    api.extendTimer(t.id, m).catch((e) => ontoast(errorText(e)));
  }
  function cancel() {
    api.cancelTimer(t.id).catch((e) => ontoast(errorText(e)));
  }
</script>

{#if t}
  <div class="strip" class:ringing role={ringing ? "alert" : undefined}>
    <button class="main" onclick={onopen} title="Open timers">
      <Icon name={ringing ? "bell" : t.kind === "alarm" ? "alarm" : "timer"} size={15} />
      <span class="name">{timerTitle(t)}</span>
      {#if more > 0}<span class="more">+{more}</span>{/if}
      <span class="time">{ringing ? (t.missed ? "Missed" : "Done") : formatRemaining(t.endsMs - now)}</span>
    </button>
    {#if ringing}
      <button class="pill" onclick={() => extend(5)} title="Ring again in 5 minutes">Snooze 5</button>
      <button class="pill strong" onclick={cancel}>Dismiss</button>
    {:else}
      <button class="pill" onclick={() => extend(1)} title="Add 1 minute">+1</button>
      <button class="icon-small" onclick={cancel} aria-label="Cancel {timerTitle(t)}" title="Cancel">
        <Icon name="x" size={14} />
      </button>
    {/if}
    {#if !ringing && t.kind === "countdown"}
      <div class="bar" style:--p={progress(t, now)}></div>
    {/if}
  </div>
{/if}

<style>
  .strip {
    position: relative;
    overflow: hidden;
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 14px 8px;
    padding: 5px 6px 6px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    border: 1px solid var(--stroke);
    box-shadow: var(--shadow);
  }
  .strip.ringing {
    border-color: color-mix(in oklab, var(--warning) 50%, transparent);
    background: color-mix(in oklab, var(--warning) 20%, var(--surface));
    animation: ring 1.6s ease-in-out infinite;
  }
  @keyframes ring {
    50% {
      background: color-mix(in oklab, var(--warning) 34%, var(--surface));
    }
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    text-align: left;
  }
  .main :global(svg) {
    color: var(--accent);
  }
  .ringing .main :global(svg) {
    color: var(--warning);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
  }
  .more {
    padding: 0 6px;
    border-radius: 8px;
    background: var(--track);
    font-size: 11px;
    color: var(--text-2);
  }
  .time {
    font: 600 15px/1 var(--font-display);
    font-variant-numeric: tabular-nums;
  }
  .ringing .time {
    font-size: 12.5px;
    color: var(--warning);
  }
  .pill {
    height: 26px;
    padding: 0 9px;
    border-radius: 13px;
    background: var(--surface-strong);
    border: 1px solid var(--stroke-strong);
    font-size: 12px;
    white-space: nowrap;
  }
  .pill:hover {
    border-color: var(--accent);
  }
  .pill.strong {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .icon-small {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 13px;
    color: var(--text-2);
  }
  .icon-small:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .bar {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: calc(var(--p) * 100%);
    background: var(--accent);
    transition: width 0.25s linear;
  }
</style>

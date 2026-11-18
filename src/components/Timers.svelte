<script lang="ts">
  import { errorText, type Api } from "../lib/api";
  import { actionLabel, clockTime, formatRemaining, progress, targetsLabel, timerTitle } from "../lib/timers";
  import type { Bulb, Scene, Timer, TimerAction, TimerInput } from "../lib/types";
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    api,
    timers,
    bulbs,
    scenes,
    now,
    targets,
    targetLabel,
    effectsAcknowledged,
    onclose,
    onneedeffects,
    ontoast,
  }: {
    api: Api;
    timers: Timer[];
    bulbs: Bulb[];
    scenes: Scene[];
    /** PC clock, ticking. */
    now: number;
    /** The main view's selection, as timer targets ([] = all). */
    targets: string[];
    targetLabel: string;
    effectsAcknowledged: boolean;
    onclose: () => void;
    /** Ask for the one-time experimental-effects OK, then run `start`. */
    onneedeffects: (label: string, start: () => Promise<unknown>) => void;
    ontoast: (message: string) => void;
  } = $props();

  type ActionType = TimerAction["type"];
  const ACTIONS: { type: ActionType; label: string; icon: IconName; hint: string }[] = [
    { type: "flash", label: "Flash", icon: "zap", hint: "Ring and pulse the lights" },
    { type: "remind", label: "Remind", icon: "bell", hint: "Ring without touching the lights" },
    { type: "off", label: "Off", icon: "power", hint: "Turn the lights off, silently" },
    { type: "on", label: "On", icon: "bulb", hint: "Turn the lights on, silently" },
    { type: "sunset", label: "Sunset", icon: "sunset", hint: "Fade down to a warm glow, then off" },
    { type: "sunrise", label: "Sunrise", icon: "sunrise", hint: "Fade up to bright white" },
    { type: "scene", label: "Scene", icon: "palette", hint: "Apply a scene" },
  ];
  const PRESETS: [number, string][] = [
    [5, "5 min"],
    [10, "10 min"],
    [25, "25 min"],
    [45, "45 min"],
  ];
  const FADES = [5, 15, 30, 60];
  const NEEDS_EFFECTS: ActionType[] = ["flash", "sunset", "sunrise"];

  /** An hour from now, rounded to 5 minutes: a sensible start for the time picker. */
  function suggestedTime() {
    const d = new Date(Date.now() + 60 * 60_000);
    d.setMinutes(Math.round(d.getMinutes() / 5) * 5);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }

  let mode = $state<"countdown" | "at">("countdown");
  let minutes = $state(10);
  let at = $state(suggestedTime());
  let label = $state("");
  let actionType = $state<ActionType>("flash");
  /** Until the user picks an action, each tab has its own default. */
  let actionPicked = false;
  let sceneName = $state("");
  let fadeMinutes = $state(15);
  let starting = $state(false);

  $effect(() => {
    if (!scenes.some((s) => s.name === sceneName)) sceneName = scenes[0]?.name ?? "";
  });

  function pickMode(m: typeof mode) {
    mode = m;
    if (!actionPicked) actionType = m === "countdown" ? "flash" : "remind";
  }
  function pickAction(type: ActionType) {
    actionType = type;
    actionPicked = true;
  }

  const validMinutes = $derived(Number.isFinite(minutes) && minutes >= 1 && minutes <= 24 * 60);
  const ready = $derived(
    (mode === "countdown" ? validMinutes : /^\d{1,2}:\d{2}$/.test(at)) && (actionType !== "scene" || !!sceneName),
  );
  const touchesLights = $derived(actionType !== "remind");
  /** The target mid-sentence: "all lights", "Office", "Desk". */
  const targetText = $derived(targets.length === 0 ? "all lights" : targetLabel);
  const startLabel = $derived(
    mode === "countdown" ? (validMinutes ? `Start ${minutes}-min timer` : "Start timer") : `Set for ${at || "…"}`,
  );

  function action(): TimerAction {
    switch (actionType) {
      case "sunset":
      case "sunrise":
        return { type: actionType, minutes: fadeMinutes };
      case "scene":
        return { type: "scene", name: sceneName };
      default:
        return { type: actionType };
    }
  }

  async function start() {
    if (!ready) return;
    const input: TimerInput = {
      ...(mode === "countdown" ? { minutes } : { at }),
      label: label.trim(),
      action: action(),
      targets: touchesLights ? targets : [],
    };
    const run = async () => {
      await api.startTimer(input);
      label = "";
    };
    if (NEEDS_EFFECTS.includes(actionType) && !effectsAcknowledged) {
      onneedeffects("Start timer", run);
      return;
    }
    starting = true;
    try {
      await run();
    } catch (e) {
      ontoast(errorText(e));
    } finally {
      starting = false;
    }
  }

  function extend(t: Timer, m: number) {
    api.extendTimer(t.id, m).catch((e) => ontoast(errorText(e)));
  }
  function cancel(t: Timer) {
    api.cancelTimer(t.id).catch((e) => ontoast(errorText(e)));
  }

  function detail(t: Timer) {
    const parts: string[] = [];
    if (t.ringingSinceMs) parts.push(t.missed ? `Missed at ${clockTime(t.endsMs)}` : `Done at ${clockTime(t.endsMs)}`);
    else if (t.kind === "alarm") parts.push(`at ${clockTime(t.endsMs)}`);
    if (t.label || !t.rings) parts.push(actionLabel(t.action));
    else if (t.action.type === "flash") parts.push("Flash");
    if (t.action.type !== "remind") parts.push(targetsLabel(t.targets, bulbs));
    return parts.join(" · ");
  }
</script>

<div class="timers">
  <header>
    <button class="icon-button" onclick={onclose} aria-label="Back"><Icon name="back" /></button>
    <h1>Timers</h1>
  </header>

  <div class="scroll">
    <section>
      <div class="tabs" role="tablist" aria-label="Timer type">
        <button role="tab" aria-selected={mode === "countdown"} onclick={() => pickMode("countdown")}>
          <Icon name="timer" size={15} />Countdown
        </button>
        <button role="tab" aria-selected={mode === "at"} onclick={() => pickMode("at")}>
          <Icon name="alarm" size={15} />At time
        </button>
      </div>

      {#if mode === "countdown"}
        <div class="presets" role="radiogroup" aria-label="Length">
          {#each PRESETS as [m, text] (m)}
            <button
              role="radio"
              aria-checked={minutes === m}
              title={m === 25 ? "Pomodoro" : undefined}
              onclick={() => (minutes = m)}>{text}</button>
          {/each}
        </div>
        <label class="field inline">
          <span>Minutes</span>
          <input type="number" min="1" max="1440" step="1" bind:value={minutes} />
        </label>
      {:else}
        <label class="field inline">
          <span>Time</span>
          <input type="time" bind:value={at} required />
        </label>
      {/if}

      <label class="field">
        <span>Reminder <small>(optional)</small></span>
        <input type="text" bind:value={label} maxlength="40" placeholder="Pasta, stretch, call back…" />
      </label>

      <div class="then">
        <span class="field-label">Then</span>
        <div class="actions" role="radiogroup" aria-label="When it ends">
          {#each ACTIONS as a (a.type)}
            <button role="radio" aria-checked={actionType === a.type} title={a.hint} onclick={() => pickAction(a.type)}>
              <Icon name={a.icon} size={14} />{a.label}
            </button>
          {/each}
        </div>
      </div>

      {#if actionType === "scene"}
        <label class="field inline">
          <span>Scene</span>
          <select bind:value={sceneName}>
            {#each scenes as s (s.name)}
              <option value={s.name}>{s.name}</option>
            {/each}
          </select>
        </label>
      {:else if actionType === "sunset" || actionType === "sunrise"}
        <div class="field inline">
          <span>Fade</span>
          <div class="options" role="radiogroup" aria-label="Fade length">
            {#each FADES as f (f)}
              <button role="radio" aria-checked={fadeMinutes === f} onclick={() => (fadeMinutes = f)}>{f} min</button>
            {/each}
          </div>
        </div>
      {/if}

      <p class="hint">
        {#if actionType === "flash"}
          <Icon name="bell" size={13} />Rings: an alarm until you answer it, this popup, and a few pulses on {targetText}.
        {:else if actionType === "remind"}
          <Icon name="bell" size={13} />Rings: an alarm until you answer it, and this popup. The lights stay as they are.
        {:else}
          <Icon name="bulb" size={13} />Quietly changes {targetText} when it ends.
        {/if}
      </p>

      <button class="button primary wide" disabled={!ready || starting} onclick={start}>
        <Icon name={mode === "countdown" ? "timer" : "alarm"} size={16} />{startLabel}
      </button>
    </section>

    {#if timers.length}
      <section>
        <h2>Running</h2>
        <ul class="list">
          {#each timers as t (t.id)}
            {@const ringing = !!t.ringingSinceMs}
            <li class:ringing>
              <div class="row">
                <Icon name={ringing ? "bell" : t.kind === "alarm" ? "alarm" : "timer"} size={16} />
                <div class="text">
                  <span class="name">{timerTitle(t)}</span>
                  <small>{detail(t)}</small>
                </div>
                {#if ringing}
                  <button class="pill" onclick={() => extend(t, 5)} title="Ring again in 5 minutes">Snooze 5</button>
                  <button class="pill strong" onclick={() => cancel(t)}>Dismiss</button>
                {:else}
                  <span class="time">{formatRemaining(t.endsMs - now)}</span>
                  <button class="pill" onclick={() => extend(t, 1)} title="Add 1 minute">+1</button>
                  <button class="icon-small" onclick={() => cancel(t)} aria-label="Cancel {timerTitle(t)}" title="Cancel">
                    <Icon name="x" size={14} />
                  </button>
                {/if}
              </div>
              {#if !ringing && t.kind === "countdown"}
                <div class="bar" style:--p={progress(t, now)}></div>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  </div>
</div>

<style>
  .timers {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 14px 14px 6px;
  }
  h1 {
    margin: 0;
    font: 600 18px/1.2 var(--font-display);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 14px 16px;
    scrollbar-width: thin;
  }
  section {
    margin-top: 10px;
    padding: 12px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--stroke);
    display: grid;
    gap: 10px;
  }
  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    padding: 3px;
    border-radius: 10px;
    background: var(--track);
  }
  .tabs button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 30px;
    border-radius: 8px;
    font-size: 13px;
    color: var(--text-2);
    transition: background 0.15s, color 0.15s;
  }
  .tabs button[aria-selected="true"] {
    background: var(--surface-strong);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }
  .presets button {
    height: 32px;
    border-radius: 8px;
    background: var(--track);
    border: 1px solid transparent;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .presets button:hover {
    color: var(--text);
  }
  .presets button[aria-checked="true"] {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
  }
  .field {
    display: grid;
    gap: 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  .field.inline {
    grid-template-columns: 70px 1fr;
    align-items: center;
  }
  .field small {
    color: var(--text-3);
    font-size: 12px;
  }
  .field-label {
    font-size: 13px;
    color: var(--text-2);
  }
  .then {
    display: grid;
    gap: 6px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    border-radius: 9px;
    background: var(--track);
    border: 1px solid transparent;
    font-size: 12.5px;
    transition: background 0.15s, border-color 0.15s;
  }
  .actions button :global(svg) {
    color: var(--text-2);
  }
  .actions button:hover {
    background: var(--surface-hover);
  }
  .actions button[aria-checked="true"] {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .actions button[aria-checked="true"] :global(svg) {
    color: var(--accent);
  }
  .options {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 8px;
    background: var(--track);
    justify-self: start;
  }
  .options button {
    height: 26px;
    padding: 0 8px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-2);
  }
  .options button[aria-checked="true"] {
    background: var(--surface-strong);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  select {
    height: 34px;
    padding: 0 8px;
    border-radius: 8px;
    border: 1px solid var(--stroke-strong);
    background: var(--input);
  }
  select option {
    background: var(--surface-strong);
    color: var(--text);
  }
  .hint {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    margin: 0;
    font-size: 12px;
    color: var(--text-3);
  }
  .hint :global(svg) {
    margin-top: 2px;
  }
  .button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    height: 36px;
    padding: 0 14px;
    border-radius: 9px;
    font-size: 13.5px;
    font-weight: 500;
  }
  .button.primary {
    background: var(--accent);
    color: var(--accent-text);
  }
  .button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .wide {
    width: 100%;
  }
  .icon-button {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    color: var(--text-2);
  }
  .icon-button:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .list li {
    position: relative;
    overflow: hidden;
    padding: 8px 8px 9px 10px;
    border-radius: 10px;
    background: var(--track);
  }
  .list li.ringing {
    background: color-mix(in oklab, var(--warning) 20%, transparent);
    animation: ring 1.6s ease-in-out infinite;
  }
  @keyframes ring {
    50% {
      background: color-mix(in oklab, var(--warning) 34%, transparent);
    }
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row > :global(svg) {
    color: var(--text-2);
  }
  .ringing .row > :global(svg) {
    color: var(--warning);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .name {
    font-size: 13.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .text small {
    color: var(--text-3);
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    font: 600 15px/1 var(--font-display);
    font-variant-numeric: tabular-nums;
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
    height: 3px;
    width: calc(var(--p) * 100%);
    background: var(--accent);
    transition: width 0.25s linear;
  }
</style>

<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, throttle } from "./lib/api";
  import { hueSatCss, kelvinCss, TEMPERATURE_TRACK } from "./lib/color";
  import type { Bulb, HueSat, Mode, Patch, Scene, Snapshot, Theme } from "./lib/types";
  import ColorWheel from "./components/ColorWheel.svelte";
  import Icon from "./components/Icon.svelte";
  import Settings from "./components/Settings.svelte";
  import Slider from "./components/Slider.svelte";
  import Toggle from "./components/Toggle.svelte";

  const SWATCHES: HueSat[] = [
    { hue: 0, saturation: 100 },
    { hue: 24, saturation: 100 },
    { hue: 45, saturation: 100 },
    { hue: 120, saturation: 80 },
    { hue: 175, saturation: 85 },
    { hue: 215, saturation: 90 },
    { hue: 265, saturation: 80 },
    { hue: 320, saturation: 75 },
  ];
  const WHITE_PRESETS = [
    { name: "Warm", kelvin: 2700 },
    { name: "Neutral", kelvin: 4000 },
    { name: "Daylight", kelvin: 6500 },
  ];

  let snap = $state<Snapshot | null>(null);
  let view = $state<"main" | "settings">("main");
  let target = $state("all");
  let themePreview = $state<Theme | null>(null);
  let systemDark = $state(window.matchMedia("(prefers-color-scheme: dark)").matches);
  let connected = $state(true);
  let toast = $state<string | null>(null);
  let toastTimer: number | undefined;
  let spinning = $state(false);
  let namingScene = $state(false);
  let sceneName = $state("");

  // ---- selection -----------------------------------------------------------
  const bulbs = $derived(snap?.bulbs ?? []);
  const rooms = $derived([...new Set(bulbs.map((b) => b.room).filter(Boolean))].sort());
  const selected = $derived(
    target === "all"
      ? bulbs
      : target.startsWith("room:")
        ? bulbs.filter((b) => b.room === target.slice(5))
        : bulbs.filter((b) => b.ip === target),
  );
  const targetIps = $derived(selected.map((b) => b.ip));
  const reachable = $derived(selected.filter((b) => b.status === "online"));
  const primary = $derived(reachable[0] ?? selected[0] ?? null);
  const anyOn = $derived(reachable.some((b) => b.on));
  const title = $derived(
    target === "all" ? "All lights" : target.startsWith("room:") ? target.slice(5) : (primary?.name ?? "Light"),
  );
  const eyebrow = $derived.by(() => {
    if (selected.length <= 1) return [primary?.room, primary?.model].filter(Boolean).join(" · ") || "Light";
    const shared = selected.every((b) => b.room && b.room === selected[0].room) ? selected[0].room : "";
    return [shared, `${selected.length} lights`].filter(Boolean).join(" · ");
  });
  const authProblem = $derived(bulbs.find((b) => b.status === "authError") ?? null);
  const disabled = $derived(reachable.length === 0);
  const range = $derived(snap?.temperatureRange ?? [2500, 6500]);

  $effect(() => {
    if (snap && target !== "all" && selected.length === 0) target = "all";
  });

  // ---- theme ---------------------------------------------------------------
  const theme = $derived.by(() => {
    const t = themePreview ?? snap?.theme ?? "system";
    return t === "system" ? (systemDark ? "dark" : "light") : t;
  });
  $effect(() => {
    document.documentElement.dataset.theme = theme;
  });
  $effect(() => {
    document.documentElement.classList.toggle("backdrop", api.desktop && !!snap?.backdrop);
  });

  // ---- control values: follow the bulb unless the user is dragging ----------
  let brightness = $state(100);
  let temperature = $state(3000);
  let color = $state<HueSat>({ hue: 30, saturation: 80 });
  let tab = $state<Mode>("white");
  const holding = $state<Record<string, boolean>>({});
  const holdTimers: Record<string, number> = {};

  function hold(control: string) {
    clearTimeout(holdTimers[control]);
    holding[control] = true;
  }
  function release(control: string) {
    clearTimeout(holdTimers[control]);
    holdTimers[control] = window.setTimeout(() => (holding[control] = false), 500);
  }

  $effect(() => {
    const p = primary;
    if (!p) return;
    if (!holding.brightness) brightness = p.brightness;
    if (!holding.temperature) temperature = p.temperature;
    if (!holding.color) color = { hue: p.hue, saturation: p.saturation };
  });

  let modeKey = "";
  $effect(() => {
    const key = primary ? `${primary.ip}:${primary.mode}` : "";
    if (key !== modeKey) {
      modeKey = key;
      if (primary) tab = primary.mode;
    }
  });

  const lightMode = $derived(primary?.mode ?? tab);
  const light = $derived(lightMode === "color" ? hueSatCss(color) : kelvinCss(temperature));

  // ---- commands --------------------------------------------------------------
  function showToast(message: string) {
    toast = message;
    clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => (toast = null), 3200);
  }

  function send(patch: Patch, ips: string[] = targetIps) {
    if (ips.length === 0) return;
    api.command(ips, patch).catch((e) => showToast(errorText(e)));
  }

  const sendBrightness = throttle((v: number) => send({ brightness: v }), 40);
  const sendTemperature = throttle((v: number) => send({ temperature: v }), 40);
  const sendColor = throttle((c: HueSat) => send({ color: c }), 40);

  function setBrightness(v: number) {
    brightness = v;
    sendBrightness(v);
  }
  function setTemperature(v: number) {
    temperature = v;
    tab = "white";
    sendTemperature(v);
  }
  function setColor(c: HueSat) {
    color = c;
    tab = "color";
    sendColor(c);
  }
  function pickPreset(kelvin: number) {
    hold("temperature");
    setTemperature(kelvin);
    release("temperature");
  }
  function pickSwatch(c: HueSat) {
    hold("color");
    setColor(c);
    release("color");
  }

  function sceneCss(s: Scene) {
    return s.color ? hueSatCss(s.color) : kelvinCss(s.temperature ?? 3000);
  }
  function applyScene(s: Scene) {
    api.applyScene(s.name, targetIps).catch((e) => showToast(errorText(e)));
  }
  async function saveScene() {
    const name = sceneName.trim();
    if (!name) return;
    try {
      await api.saveScene(name, {
        brightness,
        color: lightMode === "color" ? color : null,
        temperature: lightMode === "white" ? temperature : null,
      });
      namingScene = false;
      sceneName = "";
      showToast(`Saved “${name}”`);
    } catch (e) {
      showToast(errorText(e));
    }
  }
  function deleteScene(s: Scene) {
    api.deleteScene(s.name).catch((e) => showToast(errorText(e)));
  }

  function refresh() {
    spinning = true;
    setTimeout(() => (spinning = false), 700);
    api.refresh().catch((e) => showToast(errorText(e)));
  }

  function bulbLine(b: Bulb) {
    if (b.status === "connecting") return "Connecting…";
    if (b.status !== "online") return b.error ?? "Offline";
    return b.on ? `${b.brightness}%` : "Off";
  }

  const statusLine = $derived.by(() => {
    if (!snap) return "Connecting…";
    if (selected.length === 0) return "No lights";
    const offline = selected.filter((b) => b.status === "offline" || b.status === "authError");
    if (reachable.length === 0) {
      return selected.some((b) => b.status === "connecting") ? "Connecting…" : (offline[0]?.error ?? "Offline");
    }
    const on = reachable.filter((b) => b.on).length;
    let text =
      selected.length === 1 && primary ? (primary.on ? `On · ${primary.brightness}%` : "Off") : `${on} of ${selected.length} on`;
    if (offline.length && selected.length > 1) text += ` · ${offline.length} offline`;
    return text;
  });

  onMount(() => {
    api.snapshot().then((s) => (snap = s), (e) => showToast(errorText(e)));
    const offSnapshot = api.subscribe(
      (s) => (snap = s),
      (ok) => (connected = ok),
    );
    const offNavigate = api.on("navigate", (to) => (view = to === "settings" ? "settings" : "main"));
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onMedia = (e: MediaQueryListEvent) => (systemDark = e.matches);
    media.addEventListener("change", onMedia);
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (view === "settings") view = "main";
      else api.hide();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      offSnapshot();
      offNavigate();
      media.removeEventListener("change", onMedia);
      window.removeEventListener("keydown", onKey);
    };
  });
</script>

<div class="app" class:web={!api.desktop}>
  {#if view === "settings"}
    <Settings {api} web={snap?.web ?? null} onclose={() => (view = "main")} onpreviewtheme={(t) => (themePreview = t)} />
  {:else}
    <header>
      <h1>Lights</h1>
      <div class="actions">
        <button class="icon-button" class:spin={spinning} onclick={refresh} aria-label="Refresh" title="Refresh">
          <Icon name="refresh" />
        </button>
        <button class="icon-button" onclick={() => (view = "settings")} aria-label="Settings" title="Settings">
          <Icon name="settings" />
        </button>
        {#if api.desktop}
          <button class="icon-button" onclick={() => api.hide()} aria-label="Hide" title="Hide (Esc)">
            <Icon name="x" />
          </button>
        {/if}
      </div>
    </header>

    <main class="content">
      {#if !connected}
        <div class="banner"><Icon name="alert" size={16} />Reconnecting to Tapo Desktop…</div>
      {/if}

      {#if snap && bulbs.length === 0}
        <div class="empty card">
          <div class="empty-icon"><Icon name="bulb" size={28} /></div>
          <h2>Add your first light</h2>
          <p>Enter your bulb's IP address and your Tapo account in Settings.</p>
          <button class="button primary" onclick={() => (view = "settings")}>Open settings</button>
        </div>
      {:else}
        {#if authProblem && api.desktop}
          <button class="banner warn" onclick={() => (view = "settings")}>
            <Icon name="alert" size={16} />
            <span>{authProblem.error ?? "Sign-in problem"}</span>
          </button>
        {/if}

        {#if bulbs.length > 1}
          <nav class="chips" aria-label="Choose lights">
            <button aria-pressed={target === "all"} onclick={() => (target = "all")}>All</button>
            {#if rooms.length > 1 || (rooms.length === 1 && bulbs.some((b) => b.room !== rooms[0]))}
              {#each rooms as room (room)}
                <button aria-pressed={target === `room:${room}`} onclick={() => (target = `room:${room}`)}>{room}</button>
              {/each}
            {/if}
            {#each bulbs as b (b.ip)}
              <button aria-pressed={target === b.ip} onclick={() => (target = b.ip)}>
                <span class="dot" class:off={!b.on || b.status !== "online"} class:bad={b.status === "offline" || b.status === "authError"}></span>
                {b.name}
              </button>
            {/each}
          </nav>
        {/if}

        <section class="hero card" class:lit={anyOn} style:--light={light} style:--level={anyOn ? 0.35 + brightness / 160 : 0}>
          <div class="hero-text">
            <span class="eyebrow">{eyebrow}</span>
            <h2>{title}</h2>
            <span class="status">{statusLine}</span>
          </div>
          <button
            class="power"
            class:on={anyOn}
            onclick={() => send({ on: !anyOn })}
            {disabled}
            aria-label={anyOn ? "Turn off" : "Turn on"}
            aria-pressed={anyOn}>
            <Icon name="power" size={24} />
          </button>
        </section>

        <section class="card control">
          <div class="label"><Icon name="sun" size={15} /><span>Brightness</span><b>{brightness}%</b></div>
          <Slider
            label="Brightness"
            value={brightness}
            min={1}
            max={100}
            track="var(--track)"
            fill="linear-gradient(90deg, color-mix(in oklab, {light} 55%, transparent), {light})"
            {disabled}
            oninput={setBrightness}
            onstart={() => hold("brightness")}
            onend={() => release("brightness")} />
        </section>

        <section class="card control">
          <div class="tabs" role="tablist" aria-label="Light mode">
            <button role="tab" aria-selected={tab === "white"} onclick={() => (tab = "white")}>
              <Icon name="thermometer" size={15} />White
            </button>
            <button role="tab" aria-selected={tab === "color"} onclick={() => (tab = "color")}>
              <Icon name="palette" size={15} />Color
            </button>
          </div>

          {#if tab === "white"}
            <div class="label"><span>Temperature</span><b>{temperature} K</b></div>
            <Slider
              label="White temperature"
              value={temperature}
              min={range[0]}
              max={range[1]}
              step={50}
              track={TEMPERATURE_TRACK}
              {disabled}
              oninput={setTemperature}
              onstart={() => hold("temperature")}
              onend={() => release("temperature")} />
            <div class="presets">
              {#each WHITE_PRESETS as p (p.kelvin)}
                <button {disabled} onclick={() => pickPreset(p.kelvin)} aria-pressed={lightMode === "white" && Math.abs(temperature - p.kelvin) < 60}>
                  <span class="swatch" style:background={kelvinCss(p.kelvin)}></span>{p.name}
                </button>
              {/each}
            </div>
          {:else}
            <div class="color">
              <ColorWheel
                hue={color.hue}
                saturation={color.saturation}
                {disabled}
                oninput={setColor}
                onstart={() => hold("color")}
                onend={() => release("color")} />
              <div class="swatches">
                {#each SWATCHES as s (s.hue)}
                  <button
                    class="swatch big"
                    style:background={hueSatCss(s)}
                    {disabled}
                    aria-label="Hue {s.hue}"
                    onclick={() => pickSwatch(s)}></button>
                {/each}
              </div>
            </div>
          {/if}
        </section>

        <section class="card">
          <div class="section-head">
            <h3>Scenes</h3>
            {#if !namingScene}
              <button class="link" {disabled} onclick={() => (namingScene = true)}><Icon name="plus" size={14} />Save current</button>
            {/if}
          </div>
          {#if namingScene}
            <form
              class="name-scene"
              onsubmit={(e) => {
                e.preventDefault();
                saveScene();
              }}>
              <!-- svelte-ignore a11y_autofocus -->
              <input type="text" bind:value={sceneName} placeholder="Scene name" maxlength="32" autofocus />
              <button class="button primary" type="submit">Save</button>
              <button class="icon-button" type="button" aria-label="Cancel" onclick={() => (namingScene = false)}><Icon name="x" size={16} /></button>
            </form>
          {/if}
          <div class="scenes">
            {#each snap?.scenes ?? [] as s (s.name)}
              <div class="scene">
                <button {disabled} onclick={() => applyScene(s)} title={s.name}>
                  <span class="swatch" style:background={sceneCss(s)}></span>
                  <span class="scene-name">{s.name}</span>
                </button>
                {#if !s.builtin}
                  <button class="remove" aria-label="Delete scene {s.name}" onclick={() => deleteScene(s)}><Icon name="x" size={12} /></button>
                {/if}
              </div>
            {/each}
          </div>
        </section>

        {#if selected.length > 1}
          <section class="card">
            <h3>Lights</h3>
            <ul class="bulbs">
              {#each selected as b (b.ip)}
                <li>
                  <button class="bulb" onclick={() => (target = b.ip)}>
                    <span
                      class="dot big"
                      class:off={!b.on || b.status !== "online"}
                      class:bad={b.status === "offline" || b.status === "authError"}
                      style:--light={b.mode === "color" ? hueSatCss(b) : kelvinCss(b.temperature)}></span>
                    <span class="bulb-text">
                      <span>{b.name}</span>
                      <small>{b.room ? `${b.room} · ` : ""}{bulbLine(b)}</small>
                    </span>
                  </button>
                  <Toggle
                    checked={b.on && b.status === "online"}
                    disabled={b.status !== "online"}
                    label="{b.name} power"
                    onchange={(v) => send({ on: v }, [b.ip])} />
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      {/if}
    </main>

    {#if api.desktop && snap?.web.running}
      <footer>
        <button class="link" onclick={() => api.openWeb()}>
          <Icon name="globe" size={14} />Web page{snap.web.lan ? " · network" : ""}<Icon name="external" size={12} />
        </button>
      </footer>
    {/if}
  {/if}

  {#if toast}
    <div class="toast" role="status">{toast}</div>
  {/if}
</div>

<style>
  .app {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh;
  }
  .app.web {
    max-width: 460px;
    margin: 0 auto;
    padding-top: env(safe-area-inset-top);
    padding-bottom: env(safe-area-inset-bottom);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 10px 8px 18px;
  }
  h1 {
    margin: 0;
    font: 600 20px/1.2 var(--font-display);
    letter-spacing: -0.01em;
  }
  .actions {
    display: flex;
    gap: 2px;
  }
  .content {
    flex: 1;
    overflow-y: auto;
    padding: 2px 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    scrollbar-width: thin;
    scrollbar-color: transparent transparent;
  }
  .content:hover {
    scrollbar-color: var(--stroke-strong) transparent;
  }
  .card {
    padding: 12px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--stroke);
    box-shadow: var(--shadow);
  }
  h3 {
    margin: 0 0 10px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-3);
  }

  /* chips */
  .chips {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    scrollbar-width: none;
    margin: 0 -14px;
    padding: 0 14px;
    flex: none;
  }
  .chips button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    height: 30px;
    padding: 0 12px;
    border-radius: 15px;
    background: var(--surface);
    border: 1px solid var(--stroke);
    color: var(--text-2);
    font-size: 13px;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }
  .chips button:hover {
    background: var(--surface-hover);
  }
  .chips button[aria-pressed="true"] {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--light, #f5c35a);
    box-shadow: 0 0 6px var(--light, #f5c35a);
    flex: none;
  }
  .dot.off {
    background: var(--text-3);
    box-shadow: none;
    opacity: 0.6;
  }
  .dot.bad {
    background: var(--danger);
    box-shadow: none;
    opacity: 1;
  }
  .dot.big {
    width: 12px;
    height: 12px;
  }

  /* hero */
  .hero {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px;
    overflow: hidden;
    flex: none;
    isolation: isolate;
  }
  .hero::before {
    content: "";
    position: absolute;
    inset: -70% -20% -70% 45%;
    background: radial-gradient(closest-side, var(--light), transparent);
    opacity: var(--level);
    filter: blur(8px);
    transition: opacity 0.35s var(--ease);
    z-index: -1;
  }
  .hero-text {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 1px;
  }
  .eyebrow {
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-2);
  }
  .hero h2 {
    margin: 0;
    font: 600 22px/1.2 var(--font-display);
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status {
    font-size: 13px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .power {
    display: grid;
    place-items: center;
    flex: none;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    background: var(--track);
    border: 1px solid var(--stroke-strong);
    color: var(--text-2);
    transition:
      background 0.25s var(--ease),
      color 0.25s,
      transform 0.15s var(--ease),
      box-shadow 0.25s;
  }
  .power:hover:not(:disabled) {
    transform: scale(1.04);
  }
  .power:active:not(:disabled) {
    transform: scale(0.95);
  }
  .power.on {
    background: var(--light);
    border-color: transparent;
    color: #1d1a14;
    box-shadow: 0 0 24px color-mix(in oklab, var(--light) 60%, transparent);
  }
  .power:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* controls */
  .control {
    display: grid;
    gap: 10px;
  }
  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-2);
  }
  .label b {
    margin-left: auto;
    font-weight: 600;
    color: var(--text);
    font-variant-numeric: tabular-nums;
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
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }
  .presets button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 32px;
    border-radius: 8px;
    background: var(--track);
    font-size: 12.5px;
    color: var(--text-2);
    border: 1px solid transparent;
  }
  .presets button:hover:not(:disabled) {
    color: var(--text);
  }
  .presets button[aria-pressed="true"] {
    border-color: var(--accent);
    color: var(--text);
  }
  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    flex: none;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }
  .color {
    display: flex;
    align-items: center;
    gap: 14px;
    justify-content: center;
    padding: 4px 0;
  }
  .swatches {
    display: grid;
    grid-template-columns: repeat(2, 30px);
    gap: 8px;
  }
  .swatch.big {
    width: 30px;
    height: 30px;
    transition: transform 0.15s var(--ease);
  }
  .swatch.big:hover:not(:disabled) {
    transform: scale(1.1);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  /* scenes */
  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .scenes {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }
  .scene {
    position: relative;
  }
  .scene > button:first-child {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    height: 36px;
    padding: 0 8px;
    border-radius: 9px;
    background: var(--track);
    font-size: 12.5px;
    color: var(--text);
    transition: background 0.15s, transform 0.12s var(--ease);
  }
  .scene > button:first-child:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .scene > button:first-child:active:not(:disabled) {
    transform: scale(0.97);
  }
  .scene-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .remove {
    position: absolute;
    top: -5px;
    right: -5px;
    display: none;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--surface-strong);
    border: 1px solid var(--stroke-strong);
    color: var(--text-2);
  }
  .scene:hover .remove,
  .remove:focus-visible {
    display: grid;
  }
  .name-scene {
    display: flex;
    gap: 6px;
    margin-bottom: 8px;
  }

  /* bulbs */
  .bulbs {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .bulbs li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 46px;
  }
  .bulbs li + li {
    border-top: 1px solid var(--stroke);
  }
  .bulb {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    padding: 6px 0;
  }
  .bulb-text {
    display: grid;
    min-width: 0;
  }
  .bulb-text small {
    color: var(--text-3);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* misc */
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    color: var(--text);
    font-size: 13px;
    text-align: left;
  }
  .banner.warn {
    background: color-mix(in oklab, var(--warning) 16%, transparent);
  }
  .banner span {
    min-width: 0;
  }
  .empty {
    display: grid;
    justify-items: center;
    gap: 6px;
    padding: 28px 18px;
    text-align: center;
  }
  .empty h2 {
    margin: 4px 0 0;
    font: 600 17px var(--font-display);
  }
  .empty p {
    margin: 0 0 8px;
    color: var(--text-2);
    font-size: 13px;
  }
  .empty-icon {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent);
  }
  footer {
    display: flex;
    justify-content: center;
    padding: 4px 14px 12px;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--accent);
    font-size: 12.5px;
    font-weight: 500;
  }
  .link:hover:not(:disabled) {
    text-decoration: underline;
  }
  .button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 34px;
    padding: 0 14px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
  }
  .button.primary {
    background: var(--accent);
    color: var(--accent-text);
  }
  .icon-button {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    color: var(--text-2);
    transition: background 0.15s, color 0.15s;
  }
  .icon-button:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .icon-button.spin :global(svg) {
    animation: spin 0.7s var(--ease);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .toast {
    position: absolute;
    left: 50%;
    bottom: 16px;
    max-width: calc(100% - 32px);
    padding: 9px 14px;
    border-radius: 10px;
    background: var(--surface-strong);
    border: 1px solid var(--stroke-strong);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    font-size: 13px;
    transform: translateX(-50%);
    animation: rise 0.2s var(--ease);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 8px);
    }
  }
</style>

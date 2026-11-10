<script lang="ts">
  import { onMount } from "svelte";
  import { errorText, type Api } from "../lib/api";
  import type { Device, SettingsUpdate, SettingsView, Theme, WebStatus } from "../lib/types";
  import Icon from "./Icon.svelte";
  import Toggle from "./Toggle.svelte";

  let {
    api,
    web,
    onclose,
    onpreviewtheme,
  }: {
    api: Api;
    web: WebStatus | null;
    onclose: () => void;
    onpreviewtheme: (theme: Theme | null) => void;
  } = $props();

  let loaded = $state<SettingsView | null>(null);
  let devices = $state<Device[]>([]);
  let theme = $state<Theme>("system");
  let username = $state("");
  let tapoPassword = $state("");
  let startMinimized = $state(false);
  let launchOnStartup = $state(false);
  let webEnabled = $state(false);
  let webPort = $state(4530);
  let webLan = $state(false);
  let webPassword = $state("");
  let removeWebPassword = $state(false);

  let newIp = $state("");
  let newName = $state("");
  let newRoom = $state("");
  let addError = $state<string | null>(null);

  let error = $state<string | null>(null);
  let saving = $state(false);
  let savedAt = $state(0);
  let copied = $state<string | null>(null);

  const IPV4 = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}$/;

  function load(s: SettingsView) {
    loaded = s;
    devices = s.devices.map((d) => ({ ...d }));
    theme = s.theme;
    username = s.username;
    startMinimized = s.startMinimized;
    launchOnStartup = s.launchOnStartup;
    webEnabled = s.web.enabled;
    webPort = s.web.port;
    webLan = s.web.lan;
    tapoPassword = "";
    webPassword = "";
    removeWebPassword = false;
  }

  onMount(() => {
    api.getSettings().then(load, (e) => (error = errorText(e)));
    return () => onpreviewtheme(null);
  });

  function addDevice() {
    const ip = newIp.trim();
    if (!IPV4.test(ip) && !ip.includes(":")) {
      addError = "Enter the bulb's IP address, like 192.168.1.42";
      return;
    }
    if (devices.some((d) => d.ip === ip)) {
      addError = "That bulb is already in the list";
      return;
    }
    devices.push({ ip, name: newName.trim(), room: newRoom.trim() });
    newIp = newName = newRoom = "";
    addError = null;
  }

  function setTheme(t: Theme) {
    theme = t;
    onpreviewtheme(t);
  }

  async function save() {
    if (!loaded) return;
    saving = true;
    error = null;
    const update: SettingsUpdate = { devices, theme };
    if (loaded.desktop) {
      update.username = username.trim();
      if (tapoPassword) update.tapoPassword = tapoPassword;
      update.startMinimized = startMinimized;
      update.launchOnStartup = launchOnStartup;
      update.web = {
        enabled: webEnabled,
        port: Math.round(Number(webPort)),
        lan: webLan,
        password: removeWebPassword ? "" : webPassword || null,
      };
    }
    try {
      load(await api.saveSettings(update));
      savedAt = Date.now();
      setTimeout(() => (savedAt = 0), 1800);
    } catch (e) {
      error = errorText(e);
    } finally {
      saving = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = text;
      setTimeout(() => (copied = null), 1500);
    } catch {
      /* clipboard unavailable */
    }
  }

  const themes = [
    ["system", "monitor", "System"],
    ["light", "sun", "Light"],
    ["dark", "moon", "Dark"],
  ] as const;

  const webUrls = $derived([web?.localUrl, web?.lanUrl].filter((u): u is string => !!u));

  // A prompt to paste into an AI assistant that can make HTTP requests on this network.
  const aiPrompt = $derived.by(() => {
    const url = web?.lanUrl ?? web?.localUrl;
    if (!url) return null;
    return [
      "You can control my smart lights through TapOnLights, an app on my home network.",
      `Its API is at ${url}. First read ${url}/llms.txt: it explains everything the API can do and how to use it.`,
      "Then do what I ask by sending HTTP requests (for example with curl).",
      web?.hasPassword
        ? "It uses HTTP Basic auth: any username, password: <PASSWORD>"
        : "No password is needed.",
      "",
      "My request: ",
    ].join("\n");
  });

  const lanWithoutPassword = $derived(
    webEnabled && webLan && !webPassword && (removeWebPassword || !loaded?.web.hasPassword),
  );
</script>

<div class="settings">
  <header>
    <button class="icon-button" onclick={onclose} aria-label="Back"><Icon name="back" /></button>
    <h1>Settings</h1>
  </header>

  {#if !loaded}
    <p class="muted pad">{error ?? "Loading…"}</p>
  {:else}
    <div class="scroll">
      <section>
        <h2>Lights</h2>
        {#if devices.length === 0}
          <p class="muted">No bulbs yet. Add one by its IP address (your router's device list or the Tapo app's device info shows it).</p>
        {/if}
        <ul class="devices">
          {#each devices as device, i (device.ip)}
            <li>
              <div class="device-fields">
                <input type="text" bind:value={device.name} placeholder="Name" aria-label="Name for {device.ip}" />
                <input type="text" bind:value={device.room} placeholder="Room" aria-label="Room for {device.ip}" />
              </div>
              <div class="device-meta">
                <span class="ip">{device.ip}</span>
                <button class="icon-button small" aria-label="Remove {device.ip}" onclick={() => devices.splice(i, 1)}>
                  <Icon name="trash" size={15} />
                </button>
              </div>
            </li>
          {/each}
        </ul>
        <form
          class="add"
          onsubmit={(e) => {
            e.preventDefault();
            addDevice();
          }}>
          <input type="text" bind:value={newIp} placeholder="IP address" aria-label="New bulb IP address" />
          <input type="text" bind:value={newName} placeholder="Name" aria-label="New bulb name" />
          <input type="text" bind:value={newRoom} placeholder="Room" aria-label="New bulb room" />
          <button class="button" type="submit"><Icon name="plus" size={16} />Add</button>
        </form>
        {#if addError}<p class="error">{addError}</p>{/if}
      </section>

      {#if loaded.desktop}
        <section>
          <h2>Tapo account</h2>
          <label class="field">
            <span>Email</span>
            <input type="email" bind:value={username} placeholder="you@example.com" autocomplete="username" />
          </label>
          <label class="field">
            <span>Password</span>
            <input
              type="password"
              bind:value={tapoPassword}
              placeholder={loaded.hasTapoPassword ? "Saved · type to replace" : "Tapo account password"}
              autocomplete="current-password" />
          </label>
          <p class="hint"><Icon name="lock" size={13} />Stored in Windows Credential Manager, never in the settings file.</p>
        </section>
      {/if}

      <section>
        <h2>Appearance</h2>
        <div class="segmented" role="radiogroup" aria-label="Theme">
          {#each themes as [value, icon, text] (value)}
            <button role="radio" aria-checked={theme === value} onclick={() => setTheme(value)}>
              <Icon name={icon} size={15} />{text}
            </button>
          {/each}
        </div>
      </section>

      {#if loaded.desktop}
        <section>
          <h2>Startup</h2>
          <div class="row">
            <span>Launch when I sign in to Windows</span>
            <Toggle checked={launchOnStartup} label="Launch at sign-in" onchange={(v) => (launchOnStartup = v)} />
          </div>
          <div class="row">
            <span>Start hidden in the tray</span>
            <Toggle checked={startMinimized} label="Start hidden" onchange={(v) => (startMinimized = v)} />
          </div>
        </section>

        <section>
          <h2>Web page</h2>
          <div class="row">
            <span>
              Control lights from a browser
              <small>Same controls as this panel, served by this PC.</small>
            </span>
            <Toggle checked={webEnabled} label="Enable web page" onchange={(v) => (webEnabled = v)} />
          </div>
          {#if webEnabled}
            <div class="row">
              <span>
                Allow other devices on my network
                <small>Phones and other computers can open it. Off: this PC only.</small>
              </span>
              <Toggle checked={webLan} label="Allow network access" onchange={(v) => (webLan = v)} />
            </div>
            <label class="field inline">
              <span>Port</span>
              <input type="number" min="1024" max="65535" bind:value={webPort} />
            </label>
            <label class="field">
              <span>Password <small>(optional)</small></span>
              <input
                type="password"
                bind:value={webPassword}
                disabled={removeWebPassword}
                placeholder={loaded.web.hasPassword ? "Set · type to change" : "No password"}
                autocomplete="new-password" />
            </label>
            {#if loaded.web.hasPassword}
              <div class="row">
                <span>Remove the password</span>
                <Toggle checked={removeWebPassword} label="Remove password" onchange={(v) => (removeWebPassword = v)} />
              </div>
            {/if}
            {#if lanWithoutPassword}
              <p class="warning"><Icon name="alert" size={14} />Anyone on your network could control the lights. Consider a password.</p>
            {/if}
            {#if web?.running}
              <div class="urls">
                {#each webUrls as url (url)}
                  <div class="url">
                    <code>{url}</code>
                    <button class="icon-button small" aria-label="Copy {url}" onclick={() => copy(url)}>
                      <Icon name={copied === url ? "check" : "copy"} size={15} />
                    </button>
                  </div>
                {/each}
                {#if web.lan && !web.lanUrl}<p class="hint">Network address not detected. Use this PC's IP address with the port above.</p>{/if}
              </div>
              {#if aiPrompt}
                <button class="button ai" onclick={() => copy(aiPrompt)}>
                  <Icon name={copied === aiPrompt ? "check" : "copy"} size={15} />
                  {copied === aiPrompt ? "Prompt copied" : "Copy prompt for an AI assistant"}
                </button>
                <p class="hint">
                  For assistants that can make network requests from your computer or network, like Claude Code. Web chats
                  can't reach your home network.{web.hasPassword ? " Replace <PASSWORD> with the web page password." : ""}
                </p>
              {/if}
            {:else if web?.error}
              <p class="error">{web.error}</p>
            {/if}
            <p class="hint">Username can be anything; only the password is checked. Account and web settings can only be changed here, not from the web page.</p>
          {/if}
        </section>
      {:else}
        <p class="hint pad">Account, startup and web-access settings can be changed only in the desktop app.</p>
      {/if}

      {#if loaded.desktop}
        <section class="danger-zone">
          <button class="button ghost" onclick={() => api.quit()}><Icon name="power" size={15} />Quit TapOnLights</button>
        </section>
      {/if}
    </div>

    <footer>
      {#if error}<p class="error grow">{error}</p>{:else if savedAt}<p class="saved grow"><Icon name="check" size={15} />Saved</p>{:else}<span class="grow"></span>{/if}
      <button class="button ghost" onclick={onclose}>Close</button>
      <button class="button primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
    </footer>
  {/if}
</div>

<style>
  .settings {
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
  }
  h2 {
    margin: 0 0 10px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .muted {
    color: var(--text-2);
    margin: 0 0 10px;
    font-size: 13px;
  }
  .pad {
    padding: 0 16px;
  }
  .devices {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .devices li {
    display: grid;
    gap: 6px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--stroke);
  }
  .device-fields {
    display: grid;
    grid-template-columns: 1.3fr 1fr;
    gap: 6px;
  }
  .device-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .ip {
    font: 12px ui-monospace, "Cascadia Mono", Consolas, monospace;
    color: var(--text-3);
  }
  .add {
    display: grid;
    grid-template-columns: 1.2fr 1fr 1fr auto;
    gap: 6px;
  }
  .field {
    display: grid;
    gap: 4px;
    margin-bottom: 10px;
    font-size: 13px;
    color: var(--text-2);
  }
  .field.inline {
    grid-template-columns: 1fr 110px;
    align-items: center;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 40px;
    font-size: 13.5px;
  }
  .row + .row {
    border-top: 1px solid var(--stroke);
  }
  .row small,
  .field small {
    display: block;
    color: var(--text-3);
    font-size: 12px;
  }
  .field small {
    display: inline;
  }
  .segmented {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 4px;
    padding: 3px;
    border-radius: 10px;
    background: var(--track);
  }
  .segmented button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 32px;
    border-radius: 8px;
    color: var(--text-2);
    font-size: 13px;
    transition: background 0.15s, color 0.15s;
  }
  .segmented button[aria-checked="true"] {
    background: var(--surface-strong);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  .hint {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--text-3);
  }
  .hint :global(svg) {
    margin-top: 2px;
  }
  .warning,
  .error {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    margin: 8px 0 0;
    font-size: 12.5px;
    color: var(--warning);
  }
  .error {
    color: var(--danger);
  }
  .warning :global(svg) {
    margin-top: 2px;
  }
  .urls {
    display: grid;
    gap: 6px;
    margin-top: 10px;
  }
  .url {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 6px 6px 10px;
    border-radius: 8px;
    background: var(--accent-soft);
  }
  code {
    font: 12.5px ui-monospace, "Cascadia Mono", Consolas, monospace;
    user-select: text;
  }
  .button.ai {
    width: 100%;
    margin-top: 10px;
  }
  .danger-zone {
    background: transparent;
    border: 0;
    padding: 4px 0 0;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px 14px;
    border-top: 1px solid var(--stroke);
  }
  .grow {
    flex: 1;
    margin: 0;
    min-width: 0;
  }
  .saved {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--success);
    font-size: 13px;
  }
  .button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 34px;
    padding: 0 14px;
    border-radius: 8px;
    background: var(--surface-strong);
    border: 1px solid var(--stroke-strong);
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
  }
  .button:hover {
    background: var(--surface-hover);
  }
  .button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .button.primary:disabled {
    opacity: 0.6;
  }
  .button.ghost {
    background: transparent;
  }
  .button.ghost:hover {
    background: var(--surface-hover);
  }
  .icon-button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    color: var(--text-2);
  }
  .icon-button:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .icon-button.small {
    width: 28px;
    height: 28px;
  }
  input:disabled {
    opacity: 0.5;
  }
</style>

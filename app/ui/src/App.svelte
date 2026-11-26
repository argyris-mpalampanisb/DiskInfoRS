<script lang="ts">
  import { onMount } from 'svelte';
  import MenuBar, { type Menu, type MenuItem } from './components/MenuBar.svelte';
  import GraphDialog from './components/GraphDialog.svelte';
  import AlarmDialog from './components/AlarmDialog.svelte';
  import { api } from './lib/api';
  import * as f from './lib/format';
  import type { Attribute, Disk, HealthConfig, Snapshot } from './lib/types';

  let snap = $state<Snapshot | null>(null);
  let sel = $state(0);
  let busy = $state(false);
  let elevating = $state(false);
  let toast = $state<string | null>(null);
  let graph = $state<{ disk: number; series: string } | null>(null);
  let alarmOpen = $state(false);
  let aboutOpen = $state(false);

  const disks = $derived(snap?.disks ?? []);
  const disk = $derived<Disk | null>(disks[sel] ?? null);
  const s = $derived(snap?.settings);
  const nvme = $derived(disk?.kind === 'Nvme');

  let toastTimer: ReturnType<typeof setTimeout>;
  function notify(msg: string) {
    toast = msg;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 4000);
  }

  function apply(next: Snapshot) {
    // Keep the same physical disk selected across refreshes.
    const cur = disk?.id;
    snap = next;
    const i = next.disks.findIndex((d) => d.id === cur);
    sel = i >= 0 ? i : Math.min(sel, Math.max(0, next.disks.length - 1));
    if (next.error) notify(next.error);
  }

  async function run(fn: () => Promise<Snapshot>) {
    busy = true;
    try {
      apply(await fn());
    } catch (e) {
      notify(String(e));
    } finally {
      busy = false;
    }
  }

  const refresh = () => run(api.refresh);

  async function elevate() {
    elevating = true;
    await run(api.elevate);
    elevating = false;
  }

  async function saveSettings(patch: Partial<NonNullable<typeof s>>) {
    if (!s) return;
    await run(() => api.setSettings({ ...s, ...patch }));
  }

  async function copyInfo() {
    if (!disk || !s) return;
    try {
      await api.copy(f.report(disk, s));
      notify('Disk information copied to clipboard');
    } catch (e) {
      notify(`Copy failed: ${e}`);
    }
  }

  function openGraph(series = 'Temperature') {
    graph = { disk: sel, series };
  }

  function attrSeries(a: Attribute): string {
    if (!disk) return 'Temperature';
    if (disk.kind === 'Nvme') {
      return ({ 2: 'Temperature', 3: 'ReallocatedSectorsCount', 5: 'Life', 6: 'HostReads', 7: 'HostWrites', 11: 'PowerOnCount', 12: 'PowerOnHours' } as Record<number, string>)[a.id] ?? 'Temperature';
    }
    return a.id.toString(16).toUpperCase().padStart(2, '0');
  }

  function alarmFor(d: Disk): HealthConfig {
    return s?.alarms[f.historyKey(d)] ?? { reallocated: 1, pending: 1, uncorrectable: 1, life_caution: 10 };
  }

  const INTERVALS = [0, 1, 3, 5, 10, 30, 60, 120, 180, 360, 720, 1440];
  const intervalLabel = (m: number) => (m === 0 ? 'Disable' : m < 60 ? `${m} min` : `${m / 60} hour${m > 60 ? 's' : ''}`);

  const menus = $derived<Menu[]>([
    {
      label: 'File',
      items: [
        { label: 'Refresh', shortcut: 'F5', action: refresh, disabled: busy },
        { label: 'Rescan (elevate)', action: elevate, disabled: snap?.mode !== 'readonly' },
        { sep: true },
        { label: 'Exit', shortcut: 'Ctrl+Q', action: () => api.exit() },
      ],
    },
    {
      label: 'Edit',
      items: [{ label: 'Copy disk info', shortcut: 'Ctrl+Shift+C', action: copyInfo, disabled: !disk }],
    },
    {
      label: 'Function',
      items: [
        { label: 'Graph', shortcut: 'Ctrl+G', action: () => openGraph(), disabled: !disk },
        {
          label: 'Auto refresh',
          sub: INTERVALS.map(
            (m): MenuItem => ({ label: intervalLabel(m), checked: s?.autoRefresh === m, action: () => saveSettings({ autoRefresh: m }) }),
          ),
        },
        { label: 'Alarm thresholds…', action: () => (alarmOpen = true), disabled: !disk },
      ],
    },
    {
      label: 'Settings',
      items: [
        { label: 'Temperature in °F', checked: s?.fahrenheit, action: () => saveSettings({ fahrenheit: !s?.fahrenheit }) },
        { label: 'Raw values in decimal', checked: s?.rawDecimal, action: () => saveSettings({ rawDecimal: !s?.rawDecimal }) },
        { label: 'Hide serial number', checked: s?.hideSerial, action: () => saveSettings({ hideSerial: !s?.hideSerial }) },
        { label: 'Elevate on start', checked: s?.elevateOnStart, action: () => saveSettings({ elevateOnStart: !s?.elevateOnStart }) },
      ],
    },
    { label: 'Help', items: [{ label: 'About DiskInfoRS', action: () => (aboutOpen = true) }] },
  ]);

  function onKey(e: KeyboardEvent) {
    if (graph || alarmOpen || aboutOpen) return;
    if (e.key === 'F5') {
      e.preventDefault();
      refresh();
    } else if (e.ctrlKey && (e.key === 'ArrowRight' || e.key === 'ArrowLeft') && disks.length) {
      e.preventDefault();
      sel = (sel + (e.key === 'ArrowRight' ? 1 : disks.length - 1)) % disks.length;
    } else if (e.ctrlKey && e.key.toLowerCase() === 'g') {
      e.preventDefault();
      openGraph();
    } else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'c') {
      e.preventDefault();
      copyInfo();
    } else if (e.ctrlKey && e.key.toLowerCase() === 'q') {
      api.exit();
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    api.onSnapshot(apply).then((u) => (unlisten = u));
    api.snapshot().then((first) => {
      apply(first);
      const want = first.disks.findIndex((d) => d.id === first.initialDisk);
      if (want >= 0) sel = want;
      if (first.initialGraph) openGraph();
      if (first.mode === 'readonly' && first.settings.elevateOnStart) elevate();
    });
    return () => unlisten?.();
  });

  const MODE_LABEL = { demo: 'Demo data', root: 'Running as root', native: 'Full access (IOKit)', elevated: 'Elevated (helper)', readonly: 'Read-only' } as const;
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <MenuBar {menus} />

  <!-- Drive selector -->
  <div class="drives" role="tablist" aria-label="Disks">
    {#each disks as d, i (d.id)}
      <button
        class="drive c-{d.health}"
        class:selected={i === sel}
        role="tab"
        aria-selected={i === sel}
        title="{d.model} ({d.dev_path})"
        onclick={() => (sel = i)}
      >
        <span class="dl1"><span class="dot c-{d.health}"></span>{d.health}</span>
        <span class="dl2 c-{f.tempLevel(d)}">{f.temp(d.temperature_c, !!s?.fahrenheit)}</span>
        <span class="dl3">{f.shortMount(d)}</span>
      </button>
    {/each}
    {#if snap && disks.length === 0}
      <div class="nodisk">No disks found</div>
    {/if}
  </div>

  {#if disk && s}
    <main>
      <h1>
        <span class="model">{disk.model || disk.id}</span>
        <span class="size">{f.size(disk.size_bytes)}</span>
        <span class="dev">{disk.dev_path}</span>
      </h1>

      <section class="top">
        <div class="left">
          <div class="box health c-{disk.health}" title={disk.health_reasons.join('\n')}>
            <div class="cap">Health Status</div>
            <div class="big">{disk.health}</div>
            {#if disk.health_percent != null}<div class="pct">{disk.health_percent} %</div>{/if}
          </div>
          <div class="box tempbox c-{f.tempLevel(disk)}">
            <div class="cap">Temperature</div>
            <div class="big">{f.temp(disk.temperature_c, s.fahrenheit)}</div>
          </div>
        </div>

        <div class="info">
          <div class="col">
            <div class="k">Firmware</div><div class="v">{disk.firmware || '----'}</div>
            <div class="k">Serial Number</div><div class="v">{f.serial(disk, s.hideSerial)}</div>
            <div class="k">Interface</div><div class="v">{disk.interface || '----'}</div>
            <div class="k">Transfer Mode</div><div class="v">{disk.transfer_mode ?? '----'}</div>
            <div class="k">Mount Points</div><div class="v" title={f.mounts(disk)}>{f.mounts(disk)}</div>
            <div class="k">Standard</div><div class="v">{disk.standard ?? '----'}</div>
            <div class="k">Features</div><div class="v feat" title={disk.features.join(', ')}>{disk.features.join(', ') || '----'}</div>
          </div>
          <div class="col">
            <div class="k">Total Host Reads</div><div class="v">{f.num(disk.host_reads_gb, 'GB')}</div>
            <div class="k">Total Host Writes</div><div class="v">{f.num(disk.host_writes_gb, 'GB')}</div>
            {#if disk.nand_writes_gb != null}
              <div class="k">NAND Writes</div><div class="v">{f.num(disk.nand_writes_gb, 'GB')}</div>
            {/if}
            <div class="k">Rotation Rate</div><div class="v">{f.rotation(disk.rotation_rate)}</div>
            <div class="k">Power On Count</div><div class="v">{f.num(disk.power_on_count, 'count')}</div>
            <div class="k">Power On Hours</div><div class="v">{f.num(disk.power_on_hours, 'hours')}</div>
          </div>
        </div>
      </section>

      {#if disk.smart_error && !disk.smart_available}
        <div class="banner">
          <span>{disk.smart_error}</span>
          {#if snap?.mode === 'readonly'}
            <button class="primary" onclick={elevate} disabled={elevating}>{elevating ? 'Waiting for authorization…' : 'Elevate'}</button>
          {/if}
        </div>
      {/if}

      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th class="st"></th>
              <th class="id">ID</th>
              <th>Attribute Name</th>
              {#if !nvme}
                <th class="n">Current</th>
                <th class="n">Worst</th>
                <th class="n">Threshold</th>
              {/if}
              <th class="raw">Raw Values</th>
            </tr>
          </thead>
          <tbody>
            {#each disk.attributes as a (a.id)}
              <tr ondblclick={() => openGraph(attrSeries(a))} title="Double-click to show the history graph">
                <td class="st"><span class="dot c-{a.status}"></span></td>
                <td class="id">{a.id.toString(16).toUpperCase().padStart(2, '0')}</td>
                <td class="name">{a.name}</td>
                {#if !nvme}
                  <td class="n">{a.current ?? '-'}</td>
                  <td class="n">{a.worst ?? '-'}</td>
                  <td class="n">{a.threshold ?? '-'}</td>
                {/if}
                <td class="raw">{f.raw(a, s.rawDecimal)}</td>
              </tr>
            {/each}
            {#if disk.attributes.length === 0}
              <tr><td colspan="7" class="none">No S.M.A.R.T. attributes</td></tr>
            {/if}
          </tbody>
        </table>
      </div>
    </main>
  {:else}
    <main class="loading">{snap ? '' : 'Scanning disks…'}</main>
  {/if}

  <footer>
    {#if snap}
      <span class="badge m-{snap.mode}">{MODE_LABEL[snap.mode]}</span>
      {#if snap.mode === 'readonly'}
        <button class="link" onclick={elevate} disabled={elevating}>{elevating ? 'Authorizing…' : 'Elevate to read S.M.A.R.T.'}</button>
      {/if}
      <span class="muted">Auto refresh: {intervalLabel(snap.settings.autoRefresh)}</span>
      <span class="muted">Last: {snap.lastRefresh ?? '—'}</span>
      <span class="grow"></span>
      <span class="muted path" title="Settings and history folder"><bdi>{snap.dataDir}</bdi></span>
      <button class="refresh" onclick={refresh} disabled={busy} title="Refresh (F5)" aria-label="Refresh">⟳</button>
    {/if}
  </footer>

  {#if toast}
    <div class="toast" role="status">{toast}</div>
  {/if}
</div>

{#if graph && s}
  <GraphDialog {disks} initialDisk={graph.disk} initialSeries={graph.series} fahrenheit={s.fahrenheit} onclose={() => (graph = null)} />
{/if}

{#if alarmOpen && disk}
  <AlarmDialog
    {disk}
    current={alarmFor(disk)}
    onclose={() => (alarmOpen = false)}
    onsave={async (c) => {
      alarmOpen = false;
      await run(() => api.setAlarm(f.historyKey(disk!), c));
    }}
  />
{/if}

{#if aboutOpen}
  <div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (aboutOpen = false)}>
    <div class="modal about" role="dialog" aria-modal="true" aria-label="About">
      <h2>DiskInfoRS {snap?.version}</h2>
      <p>S.M.A.R.T. disk health viewer for Linux and macOS, written in Rust with Tauri.</p>
      <p class="muted">
        Inspired by CrystalDiskInfo by hiyohiyo. This is an independent implementation and is not affiliated with
        Crystal Dew World.
      </p>
      <p class="muted">Data folder: {snap?.dataDir}</p>
      <div class="actions"><button onclick={() => (aboutOpen = false)}>Close</button></div>
    </div>
  </div>
{/if}

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .drives {
    display: flex;
    gap: 6px;
    padding: 8px 10px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    flex: none;
  }
  .drive {
    display: grid;
    gap: 1px;
    min-width: 92px;
    padding: 5px 10px;
    text-align: left;
    background: var(--bg);
    border: 1px solid var(--border);
  }
  .drive.selected {
    border-color: var(--c);
    background: var(--cbg);
  }
  .dl1 {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }
  .dl2 {
    color: var(--c);
    font-variant-numeric: tabular-nums;
  }
  .dl3 {
    color: var(--muted);
    font-size: 12px;
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .nodisk {
    color: var(--muted);
    padding: 8px;
  }

  main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 12px 14px 8px;
    gap: 12px;
  }
  main.loading {
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  h1 {
    margin: 0;
    font-size: 19px;
    font-weight: 600;
    display: flex;
    align-items: baseline;
    gap: 12px;
    flex-wrap: wrap;
    justify-content: center;
  }
  .size {
    color: var(--accent);
  }
  .dev {
    color: var(--muted);
    font-size: 13px;
    font-weight: 400;
  }

  .top {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 12px;
  }
  .left {
    display: grid;
    gap: 8px;
    grid-template-rows: 1fr auto;
  }
  .box {
    border-radius: 8px;
    border: 1px solid var(--c);
    background: var(--cbg);
    text-align: center;
    padding: 8px;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
  .cap {
    color: var(--muted);
    font-size: 12px;
  }
  .big {
    color: var(--c);
    font-size: 26px;
    font-weight: 700;
    line-height: 1.2;
  }
  .pct {
    color: var(--c);
    font-size: 17px;
    font-weight: 600;
  }
  .tempbox .big {
    font-size: 22px;
  }

  .info {
    display: grid;
    grid-template-columns: 1.35fr 1fr;
    gap: 6px 16px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 14px;
  }
  .col {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    align-content: start;
    min-width: 0;
  }
  .k {
    color: var(--muted);
    text-align: right;
    white-space: nowrap;
  }
  .v {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
    font-variant-numeric: tabular-nums;
  }
  .feat {
    white-space: normal;
    line-height: 1.35;
  }

  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    justify-content: space-between;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--caution-bg);
    border: 1px solid var(--caution);
  }

  .table-wrap {
    flex: 1;
    min-height: 120px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--panel);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--panel-2);
    color: var(--muted);
    font-weight: 600;
    text-align: left;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    z-index: 1;
  }
  td {
    padding: 4px 8px;
    border-bottom: 1px solid rgb(46 53 64 / 0.5);
  }
  tbody tr:hover {
    background: var(--panel-2);
  }
  .st {
    width: 26px;
    text-align: center;
  }
  .id {
    width: 38px;
    font-family: var(--mono);
  }
  .n {
    width: 78px;
    text-align: right;
  }
  .raw {
    width: 150px;
    text-align: right;
    font-family: var(--mono);
    user-select: text;
  }
  .none {
    color: var(--muted);
    text-align: center;
    padding: 20px;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 4px 10px;
    border-top: 1px solid var(--border);
    background: var(--panel);
    font-size: 12px;
    flex: none;
  }
  .badge {
    padding: 1px 8px;
    border-radius: 10px;
    font-weight: 600;
    background: var(--panel-3);
  }
  .m-root,
  .m-native,
  .m-elevated {
    background: var(--good-bg);
    color: var(--good);
  }
  .m-readonly {
    background: var(--caution-bg);
    color: var(--caution);
  }
  .m-demo {
    background: #1d3049;
    color: var(--accent);
  }
  .muted {
    color: var(--muted);
  }
  .grow {
    flex: 1;
  }
  .path {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
  }
  .link {
    background: none;
    border: none;
    color: var(--accent);
    padding: 0;
    text-decoration: underline;
  }
  .refresh {
    padding: 0 8px;
    font-size: 15px;
  }

  .toast {
    position: fixed;
    bottom: 38px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--panel-3);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 14px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.4);
    max-width: 80vw;
    z-index: 60;
  }
  .about p {
    max-width: 420px;
  }

  @media (max-width: 820px) {
    .top {
      grid-template-columns: 1fr;
    }
    .left {
      grid-template-columns: 1fr 1fr;
      grid-template-rows: auto;
    }
    .info {
      grid-template-columns: 1fr;
    }
  }
</style>

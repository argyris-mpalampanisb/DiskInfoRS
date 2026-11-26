<script lang="ts">
  import uPlot from 'uplot';
  import { onDestroy } from 'svelte';
  import { api } from '../lib/api';
  import { historyKey } from '../lib/format';
  import type { Disk, Point } from '../lib/types';

  let {
    disks,
    initialDisk,
    initialSeries,
    fahrenheit,
    onclose,
  }: { disks: Disk[]; initialDisk: number; initialSeries: string; fahrenheit: boolean; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let diskIdx = $state(initialDisk);
  // svelte-ignore state_referenced_locally
  let series = $state(initialSeries);
  let available = $state<string[]>([]);
  let points = $state<Point[]>([]);
  let host: HTMLDivElement;
  let plot: uPlot | null = null;

  const LABELS: Record<string, string> = {
    Temperature: 'Temperature',
    PowerOnHours: 'Power On Hours',
    PowerOnCount: 'Power On Count',
    Life: 'Health (Life %)',
    HostWrites: 'Host Writes (GB)',
    HostReads: 'Host Reads (GB)',
    NandWrites: 'NAND Writes (GB)',
    ReallocatedSectorsCount: 'Reallocated Sectors (raw)',
    ReallocationEventCount: 'Reallocation Events (raw)',
    CurrentPendingSectorCount: 'Pending Sectors (raw)',
    UncorrectableSectorCount: 'Uncorrectable Sectors (raw)',
  };

  const disk = $derived(disks[diskIdx]);

  function label(s: string) {
    if (LABELS[s]) return LABELS[s];
    const id = parseInt(s, 16);
    const a = disk?.attributes.find((x) => x.id === id);
    return `${s} ${a ? a.name : ''} (current)`.trim();
  }

  $effect(() => {
    const d = disks[diskIdx];
    if (!d) return;
    api.historySeries(historyKey(d)).then((list) => {
      available = list;
      if (!list.includes(series)) series = list.includes('Temperature') ? 'Temperature' : (list[0] ?? '');
    });
  });

  $effect(() => {
    const d = disks[diskIdx];
    const s = series;
    if (!d || !s) {
      points = [];
      return;
    }
    api.history(historyKey(d), s).then((p) => (points = p));
  });

  $effect(() => {
    draw(points, series, fahrenheit);
  });

  function draw(pts: Point[], s: string, f: boolean) {
    plot?.destroy();
    plot = null;
    if (!host || pts.length === 0) return;
    const conv = s === 'Temperature' && f ? (v: number) => (v * 9) / 5 + 32 : (v: number) => v;
    const xs = pts.map((p) => p.t);
    const ys = pts.map((p) => conv(p.v));
    if (xs.length === 1) {
      // A single sample: duplicate it so a line is visible.
      xs.push(xs[0] + 60);
      ys.push(ys[0]);
    }
    const axis = { stroke: '#8b95a5', grid: { stroke: '#2a303b', width: 1 }, ticks: { stroke: '#2e3540', width: 1 } };
    const unit = s === 'Temperature' ? (f ? ' °F' : ' °C') : '';
    plot = new uPlot(
      {
        width: host.clientWidth,
        height: Math.max(260, host.clientHeight),
        cursor: { drag: { x: true, y: false } },
        legend: { show: true },
        scales: { x: { time: true } },
        axes: [axis, { ...axis, values: (_u, vals) => vals.map((v) => `${v}${unit}`), size: 60 }],
        series: [
          {},
          {
            label: label(s),
            stroke: '#3aa0ff',
            width: 2,
            fill: 'rgba(58,160,255,0.12)',
            points: { show: true, size: 5, fill: '#3aa0ff' },
            value: (_u, v) => (v == null ? '--' : `${Math.round(v * 10) / 10}${unit}`),
          },
        ],
      },
      [xs, ys],
      host,
    );
  }

  function onResize() {
    if (plot && host) plot.setSize({ width: host.clientWidth, height: Math.max(260, host.clientHeight) });
  }

  onDestroy(() => plot?.destroy());
</script>

<svelte:window onresize={onResize} onkeydown={(e) => e.key === 'Escape' && onclose()} />

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal graph" role="dialog" aria-modal="true" aria-label="History graph">
    <h2>History graph</h2>
    <div class="controls">
      <select bind:value={diskIdx} aria-label="Disk">
        {#each disks as d, i}
          <option value={i}>{d.model} ({d.id})</option>
        {/each}
      </select>
      <select bind:value={series} aria-label="Series" disabled={available.length === 0}>
        {#each available as s}
          <option value={s}>{label(s)}</option>
        {/each}
      </select>
      <span class="count">{points.length} samples</span>
    </div>
    <div class="plot" bind:this={host}>
      {#if points.length === 0}
        <p class="empty">
          No history recorded for this disk yet. Values are logged on each refresh when they change.
          {#if disk && !disk.smart_available}<br />S.M.A.R.T. is unavailable for this disk, so nothing is logged.{/if}
        </p>
      {/if}
    </div>
    <div class="actions"><button onclick={onclose}>Close</button></div>
  </div>
</div>

<style>
  .graph {
    width: min(960px, calc(100vw - 40px));
    height: min(600px, calc(100vh - 40px));
    display: flex;
    flex-direction: column;
  }
  .controls {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 10px;
    flex-wrap: wrap;
  }
  .controls select {
    max-width: 340px;
  }
  .count {
    color: var(--muted);
    margin-left: auto;
  }
  .plot {
    flex: 1;
    min-height: 260px;
    position: relative;
  }
  .empty {
    color: var(--muted);
    text-align: center;
    margin-top: 90px;
  }
  .plot :global(.u-legend) {
    color: var(--text);
  }
  .plot :global(.u-select) {
    background: rgb(58 160 255 / 0.15);
  }
</style>

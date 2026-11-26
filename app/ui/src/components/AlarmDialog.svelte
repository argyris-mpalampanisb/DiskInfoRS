<script lang="ts">
  import type { Disk, HealthConfig } from '../lib/types';

  let {
    disk,
    current,
    onsave,
    onclose,
  }: { disk: Disk; current: HealthConfig; onsave: (c: HealthConfig | null) => void; onclose: () => void } = $props();

  const DEFAULT: HealthConfig = { reallocated: 1, pending: 1, uncorrectable: 1, life_caution: 10 };
  // svelte-ignore state_referenced_locally
  let c = $state<HealthConfig>({ ...current });
  const nvme = $derived(disk.kind === 'Nvme');
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label="Alarm thresholds">
    <h2>Alarm thresholds: {disk.model}</h2>
    <p class="hint">"Caution" is raised when a raw value reaches the number below. Set 0 to ignore that attribute.</p>
    <div class="grid">
      {#if !nvme}
        <label for="a05">05 Reallocated Sectors Count</label>
        <input id="a05" type="number" min="0" bind:value={c.reallocated} />
        <label for="aC5">C5 Current Pending Sector Count</label>
        <input id="aC5" type="number" min="0" bind:value={c.pending} />
        <label for="aC6">C6 Uncorrectable Sector Count</label>
        <input id="aC6" type="number" min="0" bind:value={c.uncorrectable} />
      {/if}
      <label for="life">Caution when remaining life ≤ (%)</label>
      <input id="life" type="number" min="0" max="100" bind:value={c.life_caution} />
    </div>
    <div class="actions">
      <button onclick={() => (c = { ...DEFAULT })}>Defaults</button>
      <span class="spacer"></span>
      <button onclick={onclose}>Cancel</button>
      <button
        class="primary"
        onclick={() => {
          const clean: HealthConfig = {
            reallocated: Math.max(0, Math.floor(+c.reallocated || 0)),
            pending: Math.max(0, Math.floor(+c.pending || 0)),
            uncorrectable: Math.max(0, Math.floor(+c.uncorrectable || 0)),
            life_caution: Math.min(100, Math.max(0, Math.floor(+c.life_caution || 0))),
          };
          onsave(clean);
        }}>Save</button
      >
    </div>
  </div>
</div>

<style>
  .hint {
    color: var(--muted);
    margin: -6px 0 14px;
    max-width: 440px;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 110px;
    gap: 8px 14px;
    align-items: center;
  }
  .spacer {
    flex: 1;
  }
</style>

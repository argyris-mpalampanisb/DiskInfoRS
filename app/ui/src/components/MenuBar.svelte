<script lang="ts" module>
  export type MenuItem =
    | { sep: true }
    | { label: string; shortcut?: string; checked?: boolean; disabled?: boolean; action?: () => void; sub?: MenuItem[] };
  export type Menu = { label: string; items: MenuItem[] };
</script>

<script lang="ts">
  let { menus }: { menus: Menu[] } = $props();
  let open = $state<number | null>(null);
  let subOpen = $state<number | null>(null);

  function run(item: MenuItem) {
    if ('sep' in item || item.disabled || item.sub) return;
    open = null;
    subOpen = null;
    item.action?.();
  }

  function onWindowClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest('.menubar')) {
      open = null;
      subOpen = null;
    }
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      open = null;
      subOpen = null;
    }
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

{#snippet list(items: MenuItem[], nested: boolean)}
  <div class="dropdown" class:nested role="menu">
    {#each items as item, i}
      {#if 'sep' in item}
        <div class="sep"></div>
      {:else}
        <div class="wrap" role="none" onmouseenter={() => !nested && (subOpen = item.sub ? i : null)}>
          <button class="item" role="menuitem" disabled={item.disabled} onclick={() => run(item)}>
            <span class="check">{item.checked ? '✓' : ''}</span>
            <span class="lbl">{item.label}</span>
            <span class="sc">{item.sub ? '▸' : (item.shortcut ?? '')}</span>
          </button>
          {#if item.sub && subOpen === i && !nested}
            {@render list(item.sub, true)}
          {/if}
        </div>
      {/if}
    {/each}
  </div>
{/snippet}

<nav class="menubar">
  {#each menus as m, i}
    <div class="top">
      <button
        class="title"
        class:active={open === i}
        onclick={() => {
          open = open === i ? null : i;
          subOpen = null;
        }}
        onmouseenter={() => open !== null && (open = i)}>{m.label}</button
      >
      {#if open === i}
        {@render list(m.items, false)}
      {/if}
    </div>
  {/each}
</nav>

<style>
  .menubar {
    display: flex;
    gap: 2px;
    padding: 2px 6px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }
  .top {
    position: relative;
  }
  .title {
    background: none;
    border: none;
    padding: 4px 10px;
    border-radius: 4px;
  }
  .title:hover,
  .title.active {
    background: var(--panel-3);
  }
  .dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 40;
    min-width: 230px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 12px 30px rgb(0 0 0 / 0.45);
  }
  .dropdown.nested {
    top: -5px;
    left: 100%;
    min-width: 170px;
  }
  .wrap {
    position: relative;
  }
  .item {
    display: grid;
    grid-template-columns: 18px 1fr auto;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: 5px 8px;
    border-radius: 4px;
  }
  .item:hover:not(:disabled) {
    background: var(--accent);
    color: #06121f;
  }
  .item:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .check {
    color: var(--good);
  }
  .item:hover .check {
    color: inherit;
  }
  .sc {
    color: var(--muted);
    font-size: 12px;
  }
  .item:hover .sc {
    color: inherit;
  }
  .sep {
    height: 1px;
    background: var(--border);
    margin: 4px 6px;
  }
</style>

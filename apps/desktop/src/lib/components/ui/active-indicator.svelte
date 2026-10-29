<script lang="ts">
  import { onMount } from 'svelte';
  import { Spring, prefersReducedMotion } from 'svelte/motion';
  let { selector = '[data-state="active"]' }: { selector?: string } = $props();
  let marker: HTMLSpanElement;
  let visible = $state(false);
  const position = new Spring({ x: 0, y: 0, width: 0, height: 0 }, { stiffness: 0.24, damping: 0.78 });
  onMount(() => {
    const parent = marker.parentElement!;
    let observed: HTMLElement | null = null;
    const update = () => {
      const active = parent.querySelector<HTMLElement>(selector);
      if (!active) { visible = false; return; }
      if (observed !== active) {
        if (observed) resize.unobserve(observed);
        observed = active;
        resize.observe(active);
      }
      const bounds = active.getBoundingClientRect();
      if (!bounds.width || !bounds.height) { visible = false; return; }
      const origin = parent.getBoundingClientRect();
      void position.set({ x: bounds.left - origin.left + parent.scrollLeft, y: bounds.top - origin.top + parent.scrollTop, width: bounds.width, height: bounds.height }, { instant: !visible || prefersReducedMotion.current });
      visible = true;
    };
    const resize = new ResizeObserver(update);
    const changes = new MutationObserver(update);
    resize.observe(parent);
    changes.observe(parent, { subtree: true, childList: true, attributes: true, attributeFilter: ['aria-current', 'aria-pressed', 'data-state', 'class'] });
    update();
    return () => { resize.disconnect(); changes.disconnect(); };
  });
</script>

<span bind:this={marker} class="active-indicator" aria-hidden="true" style:opacity={visible ? 1 : 0} style:width={`${position.current.width}px`} style:height={`${position.current.height}px`} style:transform={`translate(${position.current.x}px, ${position.current.y}px)`}></span>

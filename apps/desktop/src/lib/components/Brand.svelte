<script lang="ts">
  let { small = false }: { small?: boolean } = $props();
</script>

<div class:small class="brand-mark" aria-hidden="true">
  <img src="/app-icon.png" alt="" draggable="false"/>
</div>

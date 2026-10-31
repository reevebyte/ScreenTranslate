<script lang="ts">
	import { Select as SelectPrimitive } from "bits-ui";
	import { cn, type WithoutChild } from "$lib/utils";
	import type { WithoutChildrenOrChild } from "$lib/utils";
	import SelectPortal from "./select-portal.svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		class: className,
		sideOffset = 4,
		portalProps,
		children,
		preventScroll = true,
		...restProps
	}: WithoutChild<SelectPrimitive.ContentProps> & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof SelectPortal>>;
	} = $props();
</script>

<SelectPortal {...portalProps}>
	<SelectPrimitive.Content
		bind:ref
		{sideOffset}
		{preventScroll}
		data-slot="select-content"
		class={cn(
			"bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 ring-foreground/10 min-w-36 rounded-lg shadow-md ring-1 duration-100 cn-menu-target cn-menu-translucent relative z-50 max-h-(--bits-select-content-available-height) origin-(--bits-select-content-transform-origin) flex flex-col overflow-hidden",
			className
		)}
		{...restProps}
	>
		<SelectPrimitive.Viewport
			class="select-viewport w-full min-w-(--bits-select-anchor-width)"
			onpointermovecapture={event => {
				// Bits realigns the viewport whenever hover changes its highlight.
				// CSS supplies mouse hover feedback; keyboard navigation still scrolls.
				if (event.pointerType === 'mouse') event.stopPropagation();
			}}
		>
			{@render children?.()}
		</SelectPrimitive.Viewport>
	</SelectPrimitive.Content>
</SelectPortal>

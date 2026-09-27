import type { Component } from 'svelte';
import type { PluginComponentProps } from './sdk';
import Cards from './Cards.svelte';
import RoundSummary from './extensions/round-summary/Plugin.svelte';
/** Reviewed components compiled with the app, keyed by manifest.renderer. */
export const pluginComponents: Record<string, Component<PluginComponentProps>> = {
	cards: Cards,
	'round-summary': RoundSummary
};

// Expressive Code (code blocks) configuration. Kept in its own file so the <Code> component
// can load it at render time (functions in astro.config.mjs are not serialisable).
import { defineEcConfig } from '@astrojs/starlight/expressive-code';

/** Code block surfaces = the raised token colour, so Expressive Code keeps syntax colours AA. */
const CODE_BG = { dark: '#151921', light: '#f2f2ec' };

export default defineEcConfig({
	themes: ['github-dark-default', 'github-light-default'],
	customizeTheme: (theme) => {
		theme.colors['editor.background'] = theme.type === 'dark' ? CODE_BG.dark : CODE_BG.light;
		return theme;
	},
	styleOverrides: {
		borderRadius: '2px',
		borderColor: 'var(--ck-border)',
		codeFontFamily: 'var(--__sl-font-mono)',
		codeFontSize: '0.875rem',
		uiFontFamily: 'var(--__sl-font)',
		frames: {
			frameBoxShadowCssValue: 'none',
			editorTabBarBackground: 'var(--ck-surface)',
			editorActiveTabBackground: 'var(--ck-raised)',
			editorActiveTabIndicatorTopColor: 'var(--ck-accent)',
			terminalTitlebarBackground: 'var(--ck-surface)',
			terminalBackground: 'var(--ck-raised)',
			inlineButtonBorder: 'var(--ck-border)',
		},
	},
});

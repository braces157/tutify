/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./docs/**/*.{html,js}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        spotify: '#1ed760',
        'spotify-dark': '#169c46',
        terminal: '#0b0f0d',
        'terminal-card': '#101713',
        'terminal-border': '#1b2620',
      },
      fontFamily: {
        mono: ['Cascadia Code', 'Consolas', 'ui-monospace', 'monospace'],
        sans: ['Segoe UI', 'system-ui', 'sans-serif'],
      },
    },
  },
  safelist: [
    'hidden',
    'bar-anim',
    'theme-spotify',
    'theme-phosphor',
    'theme-amber',
    'theme-monochrome',
    'theme-cyberpunk',
    'is-playing',
    'is-paused',
  ],
};

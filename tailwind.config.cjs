/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./docs/**/*.{html,js}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        spotify: '#1ed760',
        'spotify-dark': '#169c46',
        terminal: '#090b0a',
        'terminal-card': '#0c0f0d',
        'terminal-border': '#242925',
      },
      fontFamily: {
        mono: ['DM Mono', 'monospace'],
        sans: ['Space Grotesk', 'sans-serif'],
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

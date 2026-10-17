/** @type {import("tailwindcss").Config} */
module.exports = {
  "content": [
    "./docs/**/*.{html,js}"
  ],
  "darkMode": "class",
  "theme": {
    "extend": {
      "colors": {
        "spotify": "#1ed760",
        "spotify-dark": "#169c46",
        "terminal": "#090b0a",
        "terminal-card": "#0c0f0d",
        "terminal-border": "#242925",
        "surface-variant": "#353534",
        "on-secondary": "#002487",
        "secondary-fixed-dim": "#b8c3ff",
        "surface-dim": "#131313",
        "editorial-blue": "#3B5BD9",
        "on-tertiary-fixed": "#241a00",
        "cadmium-amber": "#F5C518",
        "surface-container": "#201f1f",
        "on-tertiary": "#3d2f00",
        "surface-panel": "#121212",
        "on-primary-fixed": "#00210c",
        "primary": "#f1ffef",
        "primary-fixed": "#60ff99",
        "surface-bright": "#3a3939",
        "phosphor-emerald": "#10B981",
        "vermilion-flame": "#E0492A",
        "on-primary-fixed-variant": "#005228",
        "electric-neon": "#00FF88",
        "inverse-on-surface": "#313030",
        "tertiary": "#fffaf7",
        "secondary-fixed": "#dde1ff",
        "inverse-primary": "#006d37",
        "on-primary-container": "#007139",
        "surface-tint": "#00e479",
        "surface-container-lowest": "#0e0e0e",
        "on-tertiary-fixed-variant": "#584400",
        "on-surface-variant": "#b9cbb9",
        "tertiary-fixed": "#ffe08b",
        "on-error-container": "#ffdad6",
        "primary-fixed-dim": "#00e479",
        "on-surface": "#e5e2e1",
        "surface": "#131313",
        "on-secondary-fixed": "#001355",
        "on-background": "#e5e2e1",
        "secondary": "#b8c3ff",
        "secondary-container": "#0f3aba",
        "hairline-rule": "rgba(255, 255, 255, 0.12)",
        "error": "#ffb4ab",
        "error-container": "#93000a",
        "on-secondary-container": "#a5b4ff",
        "on-error": "#690005",
        "outline-variant": "#3b4b3d",
        "tertiary-container": "#ffdb75",
        "primary-container": "#00ff88",
        "surface-canvas": "#0A0A0A",
        "inverse-surface": "#e5e2e1",
        "on-primary": "#003919",
        "surface-container-highest": "#353534",
        "surface-container-high": "#2a2a2a",
        "background": "#131313",
        "tertiary-fixed-dim": "#f0c110",
        "on-secondary-fixed-variant": "#0937b8",
        "on-tertiary-container": "#785f00",
        "text-muted": "#888888",
        "outline": "#849585",
        "surface-container-low": "#1c1b1b"
      },
      "fontFamily": {
        "mono": [
          "DM Mono",
          "monospace"
        ],
        "sans": [
          "Space Grotesk",
          "sans-serif"
        ],
        "headline-xl": [
          "Space Grotesk"
        ],
        "body-sm": [
          "Work Sans"
        ],
        "mono-code": [
          "JetBrains Mono"
        ],
        "body-default": [
          "Work Sans"
        ],
        "headline-lg": [
          "Space Grotesk"
        ],
        "headline-md": [
          "Space Grotesk"
        ],
        "display": [
          "Space Grotesk"
        ],
        "headline-xl-mobile": [
          "Space Grotesk"
        ],
        "headline-sm": [
          "Space Grotesk"
        ],
        "display-mobile": [
          "Space Grotesk"
        ],
        "mono-label": [
          "JetBrains Mono"
        ],
        "body-lead": [
          "Work Sans"
        ]
      },
      "spacing": {
        "space-xs": "0.25rem",
        "space-sm": "0.5rem",
        "space-2xl": "4rem",
        "space-lg": "1.5rem",
        "space-xl": "2.5rem",
        "margin-mobile": "1.25rem",
        "space-3xl": "6rem",
        "space-md": "1rem",
        "margin": "3rem",
        "gutter-mobile": "1rem",
        "gutter": "1.5rem"
      },
      "fontSize": {
        "headline-xl": [
          "3.5rem",
          {
            "lineHeight": "1.05",
            "letterSpacing": "-0.035em",
            "fontWeight": "700"
          }
        ],
        "body-sm": [
          "0.875rem",
          {
            "lineHeight": "1.55",
            "letterSpacing": "0",
            "fontWeight": "400"
          }
        ],
        "mono-code": [
          "0.8125rem",
          {
            "lineHeight": "1.5",
            "letterSpacing": "0",
            "fontWeight": "400"
          }
        ],
        "body-default": [
          "1rem",
          {
            "lineHeight": "1.6",
            "letterSpacing": "0",
            "fontWeight": "400"
          }
        ],
        "headline-lg": [
          "2.5rem",
          {
            "lineHeight": "1.15",
            "letterSpacing": "-0.025em",
            "fontWeight": "600"
          }
        ],
        "headline-md": [
          "1.75rem",
          {
            "lineHeight": "1.25",
            "letterSpacing": "-0.02em",
            "fontWeight": "600"
          }
        ],
        "display": [
          "5rem",
          {
            "lineHeight": "0.95",
            "letterSpacing": "-0.04em",
            "fontWeight": "700"
          }
        ],
        "headline-xl-mobile": [
          "2.25rem",
          {
            "lineHeight": "1.1",
            "letterSpacing": "-0.02em",
            "fontWeight": "700"
          }
        ],
        "headline-sm": [
          "1.25rem",
          {
            "lineHeight": "1.35",
            "letterSpacing": "-0.01em",
            "fontWeight": "600"
          }
        ],
        "display-mobile": [
          "3rem",
          {
            "lineHeight": "1.0",
            "letterSpacing": "-0.03em",
            "fontWeight": "700"
          }
        ],
        "mono-label": [
          "0.75rem",
          {
            "lineHeight": "1.2",
            "letterSpacing": "0.08em",
            "fontWeight": "500"
          }
        ],
        "body-lead": [
          "1.375rem",
          {
            "lineHeight": "1.45",
            "letterSpacing": "-0.01em",
            "fontWeight": "400"
          }
        ]
      }
    }
  },
  "safelist": [
    "hidden",
    "bar-anim",
    "theme-spotify",
    "theme-phosphor",
    "theme-amber",
    "theme-monochrome",
    "theme-cyberpunk",
    "is-playing",
    "is-paused"
  ]
};

/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        surface: {
          DEFAULT: "#0b0d10",
          raised: "#111418",
          muted: "#161a1f",
        },
        border: { DEFAULT: "#1f242b" },
        text: {
          DEFAULT: "#e6e8eb",
          muted: "#9aa3ad",
          faint: "#6b727b",
        },
        accent: {
          DEFAULT: "#4f8cff",
          soft: "#1b2b4a",
        },
        danger: { DEFAULT: "#e5484d", soft: "#2a1418" },
        warn: { DEFAULT: "#f5a524", soft: "#2a1d0a" },
        ok: { DEFAULT: "#30a46c", soft: "#0f2318" },
      },
      fontFamily: {
        mono: [
          "ui-monospace",
          "SFMono-Regular",
          "Menlo",
          "Consolas",
          "monospace",
        ],
      },
    },
  },
  plugins: [],
};

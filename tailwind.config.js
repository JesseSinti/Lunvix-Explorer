/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{html,js,svelte,ts}'],
  theme: {
    extend: {
      colors: {
        base: '#08080d',      // C_BASE
        panel: '#0d0d14',     // C_PANEL
        surface: '#14141e',   // C_SURFACE
        input: '#1a1a26',     // C_INPUT
        stripe: '#0b0b11',    // C_STRIPE
        borderLo: '#1c1c2a',  // C_BORDER_LO
        borderHi: '#36364a',  // C_BORDER_HI
        accent: '#00d2ff',    // C_ACCENT
        accentDim: '#003246', // C_ACCENT_DIM
        accentMid: '#008cb4', // C_ACCENT_MID
        success: '#2ecc71',   // C_SUCCESS
        txtHi: '#e4e4f0',     // C_TXT_HI
        txtMid: '#73738c',    // C_TXT_MID
        txtLo: '#37374e',     // C_TXT_LO
        danger: '#ff4250',    // C_DANGER
        dangerBg: '#2c0e12',  // C_DANGER_BG
        dangerBd: '#581a22',  // C_DANGER_BD
      },
      fontFamily: {
        sans: ['Inter', 'sans-serif'],
        mono: ['Fira Code', 'monospace'],
      }
    }
  },
  plugins: []
};

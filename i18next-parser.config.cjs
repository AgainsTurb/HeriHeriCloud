module.exports = {
  locales: ['en', 'zh'], 
  output: 'src/locales/$LOCALE.json', 
  input: ['src/**/*.{js,jsx,ts,tsx}'], 
  sort: true, 
  createOldCatalogs: false, 
  keySeparator: false, 
  namespaceSeparator: false 
};
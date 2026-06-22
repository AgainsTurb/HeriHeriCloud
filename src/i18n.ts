import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

// Import the localized JSON files
import enTranslation from './locales/en.json';
import zhTranslation from './locales/zh.json';

const resources = {
  en: { translation: enTranslation },
  zh: { translation: zhTranslation }
};

i18n
  .use(LanguageDetector) // Automatically detects OS/Browser language
  .use(initReactI18next) // Passes i18n instance to react-i18next
  .init({
    resources,
    fallbackLng: 'en', // If a Chinese translation is missing, show English
    keySeparator: false,
    interpolation: {
      escapeValue: false // React already safely escapes values
    }
  });

export default i18n;
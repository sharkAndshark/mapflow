import React from 'react';
import { useTranslation } from 'react-i18next';

const languages = [
  { code: 'zh', name: '中文', flag: '🇨🇳' },
  { code: 'en', name: 'English', flag: '🇺🇸' },
];

export default function LanguageSwitcher() {
  const { i18n } = useTranslation();

  const toggleLanguage = () => {
    const newLang = i18n.language === 'zh' ? 'en' : 'zh';
    i18n.changeLanguage(newLang);
  };

  const currentLang = languages.find((lang) => lang.code === i18n.language) || languages[0];
  const nextLang = languages.find((lang) => lang.code !== i18n.language) || languages[1];

  return (
    <button
      type="button"
      onClick={toggleLanguage}
      className="btn-text"
      style={{ fontSize: '14px', display: 'flex', alignItems: 'center', gap: '4px' }}
      title={nextLang.name}
    >
      <span style={{ fontSize: '16px' }}>{currentLang.flag}</span>
      <span>{currentLang.code.toUpperCase()}</span>
    </button>
  );
}

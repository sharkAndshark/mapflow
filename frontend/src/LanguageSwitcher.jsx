import React from 'react';
import { useTranslation } from 'react-i18next';

const languages = [
  { code: 'zh', name: '中文', flag: '🇨🇳' },
  { code: 'en', name: 'English', flag: '🇺🇸' },
];

export default function LanguageSwitcher() {
  const { i18n } = useTranslation();
  const rawLanguage = i18n.resolvedLanguage || i18n.language || 'zh';
  const currentCode = rawLanguage.split(/[-_]/)[0].toLowerCase();

  const toggleLanguage = () => {
    const newLang = currentCode === 'zh' ? 'en' : 'zh';
    i18n.changeLanguage(newLang);
  };

  const currentLang = languages.find((lang) => lang.code === currentCode) || languages[0];
  const nextLang = languages.find((lang) => lang.code !== currentCode) || languages[1];

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

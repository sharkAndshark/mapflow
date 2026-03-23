import React from 'react';
import { useTranslation } from 'react-i18next';

export default function IconsPanel() {
  const { t } = useTranslation();

  return <div className="empty">{t('app.resourceIconsComingSoon')}</div>;
}

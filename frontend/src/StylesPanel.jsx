import React from 'react';
import { useTranslation } from 'react-i18next';

export default function StylesPanel() {
  const { t } = useTranslation();

  return <div className="empty">{t('app.resourceStylesComingSoon')}</div>;
}

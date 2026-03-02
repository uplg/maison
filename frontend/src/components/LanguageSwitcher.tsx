import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { languages } from "@/i18n";
import { Globe } from "lucide-react";

export function LanguageSwitcher() {
  const { i18n } = useTranslation();

  const currentLang = languages.find((l) => l.code === i18n.language) || languages[0];
  const nextLang = languages.find((l) => l.code !== i18n.language) || languages[1];

  const toggleLanguage = () => {
    i18n.changeLanguage(nextLang.code);
  };

  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={toggleLanguage}
      className="gap-2"
      title={`Switch to ${nextLang.name}`}
    >
      <Globe className="h-4 w-4" />
      <span>{currentLang.flag}</span>
    </Button>
  );
}

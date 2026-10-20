import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { LanguageSelector } from "../LanguageSelector";
import { TranslateToEnglish } from "../TranslateToEnglish";
import { useModelStore } from "../../../stores/modelStore";
import { useSettings } from "@/hooks/useSettings";
import type { ModelInfo } from "@/bindings";

export const ModelSettingsCard: React.FC = () => {
  const { t } = useTranslation();
  const { currentModel, models } = useModelStore();
  const { getSetting } = useSettings();
  const provider = getSetting("transcription_provider") ?? "local";

  const currentModelInfo = models.find((m: ModelInfo) => m.id === currentModel);

  const showLanguageSelector =
    currentModelInfo?.supports_language_selection ?? false;
  const supportsTranslation = currentModelInfo?.supports_translation ?? false;
  const hasAnySettings = showLanguageSelector || supportsTranslation;

  if (provider === "cartesia") return null;
  if (provider !== "local") {
    const service = t(
      `settings.general.transcriptionProvider.${provider === "warp_wispr" ? "warpWispr" : "elevenLabs"}`,
    );
    return (
      <SettingsGroup
        title={t("settings.modelSettings.title", { model: service })}
      >
        <LanguageSelector
          descriptionMode="tooltip"
          grouped={true}
          supportsLanguageDetection={true}
        />
      </SettingsGroup>
    );
  }

  // Don't render anything if no model is selected or no settings available
  if (!currentModel || !currentModelInfo || !hasAnySettings) {
    return null;
  }

  return (
    <SettingsGroup
      title={t("settings.modelSettings.title", {
        model: currentModelInfo.name,
      })}
    >
      {showLanguageSelector && (
        <LanguageSelector
          descriptionMode="tooltip"
          grouped={true}
          supportedLanguages={currentModelInfo.supported_languages}
          supportsLanguageDetection={
            currentModelInfo.supports_language_detection
          }
        />
      )}
      {supportsTranslation && (
        <TranslateToEnglish descriptionMode="tooltip" grouped={true} />
      )}
    </SettingsGroup>
  );
};

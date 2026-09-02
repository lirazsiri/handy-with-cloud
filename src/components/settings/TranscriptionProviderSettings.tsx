import { type FC, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionProvider } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Dropdown } from "@/components/ui/Dropdown";
import { Input } from "@/components/ui/Input";
import { SettingContainer } from "@/components/ui/SettingContainer";

export const TranscriptionProviderSettings: FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const provider = getSetting("transcription_provider") ?? "local";
  const persistedCredentials = getSetting("warp_credentials") ?? "";
  const [credentials, setCredentials] = useState(persistedCredentials);

  useEffect(() => setCredentials(persistedCredentials), [persistedCredentials]);

  return (
    <>
      <SettingContainer
        title={t("settings.general.transcriptionProvider.title")}
        description={t("settings.general.transcriptionProvider.description")}
        grouped={true}
      >
        <Dropdown
          options={[
            {
              value: "local",
              label: t("settings.general.transcriptionProvider.local"),
            },
            {
              value: "warp_wispr",
              label: t("settings.general.transcriptionProvider.warpWispr"),
            },
          ]}
          selectedValue={provider}
          onSelect={(value) =>
            updateSetting(
              "transcription_provider",
              value as TranscriptionProvider,
            )
          }
          disabled={isUpdating("transcription_provider")}
        />
      </SettingContainer>
      {provider === "warp_wispr" && (
        <SettingContainer
          title={t("settings.general.transcriptionProvider.credentialsTitle")}
          description={t(
            "settings.general.transcriptionProvider.credentialsDescription",
          )}
          grouped={true}
          layout="stacked"
        >
          <Input
            type="password"
            value={credentials}
            onChange={(event) => setCredentials(event.target.value)}
            onPaste={(event) => {
              const pastedCredentials = event.clipboardData.getData("text");
              if (!pastedCredentials) return;
              event.preventDefault();
              setCredentials(pastedCredentials);
              void updateSetting("warp_credentials", pastedCredentials);
            }}
            onBlur={() => updateSetting("warp_credentials", credentials)}
            placeholder={t(
              "settings.general.transcriptionProvider.credentialsPlaceholder",
            )}
            disabled={isUpdating("warp_credentials")}
            className="w-full font-mono"
          />
        </SettingContainer>
      )}
    </>
  );
};

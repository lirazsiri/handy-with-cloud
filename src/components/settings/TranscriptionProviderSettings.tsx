import { type FC, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TranscriptionProvider } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Dropdown } from "@/components/ui/Dropdown";
import { Input } from "@/components/ui/Input";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { Button } from "@/components/ui/Button";
import { Alert } from "@/components/ui/Alert";
import { commands } from "@/bindings";

import { WisprFlowSettings } from "./WisprFlowSettings";

type HealthState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "active"; source: string; latencyMs: number }
  | { kind: "error"; message: string }
  | { kind: "verified"; saved: boolean };

export const TranscriptionProviderSettings: FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const provider = getSetting("transcription_provider") ?? "local";
  const persistedCredentialsFile = getSetting("warp_credentials_file") ?? "";
  const [credentialsFile, setCredentialsFile] = useState(
    persistedCredentialsFile,
  );
  const [apiKey, setApiKey] = useState("");
  const [health, setHealth] = useState<HealthState>({ kind: "idle" });

  useEffect(
    () => setCredentialsFile(persistedCredentialsFile),
    [persistedCredentialsFile],
  );

  useEffect(() => {
    setApiKey("");
    setHealth({ kind: "idle" });
  }, [provider]);

  const checkCloudKey = async (save: boolean) => {
    setHealth({ kind: "checking" });
    try {
      const result = save
        ? await commands.saveCloudSttApiKey(provider, apiKey)
        : await commands.checkCloudSttApiKey(provider);
      if (result.status === "error") {
        setHealth({ kind: "error", message: result.error });
      } else {
        setHealth({ kind: "verified", saved: save });
        if (save) setApiKey("");
      }
    } catch (error) {
      setHealth({ kind: "error", message: String(error) });
    }
  };

  const checkConnection = async () => {
    setHealth({ kind: "checking" });
    try {
      await updateSetting("warp_credentials_file", credentialsFile);
      const result = await commands.checkWarpWisprStatus(credentialsFile);
      if (result.status === "error") {
        setHealth({ kind: "error", message: String(result.error) });
        return;
      }
      setHealth({
        kind: "active",
        source: result.data.credential_source,
        latencyMs: result.data.latency_ms,
      });
    } catch (error) {
      setHealth({ kind: "error", message: String(error) });
    }
  };

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
            {
              value: "wispr_flow",
              label: t("settings.general.transcriptionProvider.wisprFlow"),
            },
            {
              value: "cartesia",
              label: t("settings.general.transcriptionProvider.cartesia"),
            },
            {
              value: "eleven_labs",
              label: t("settings.general.transcriptionProvider.elevenLabs"),
            },
          ]}
          selectedValue={provider}
          onSelect={(value) =>
            updateSetting(
              "transcription_provider",
              value as TranscriptionProvider,
            )
          }
          disabled={
            isUpdating("transcription_provider") || health.kind === "checking"
          }
        />
      </SettingContainer>
      {provider === "wispr_flow" && <WisprFlowSettings />}
      {(provider === "cartesia" || provider === "eleven_labs") && (
        <SettingContainer
          title={t("settings.general.transcriptionProvider.apiKeyTitle")}
          description={t(
            "settings.general.transcriptionProvider.apiKeyDescription",
          )}
          grouped={true}
          layout="stacked"
        >
          <Input
            type="password"
            aria-label={t("settings.general.transcriptionProvider.apiKeyTitle")}
            value={apiKey}
            autoComplete="off"
            onChange={(event) => {
              setApiKey(event.target.value);
              setHealth({ kind: "idle" });
            }}
            placeholder={t(
              "settings.general.transcriptionProvider.apiKeyPlaceholder",
            )}
            disabled={health.kind === "checking"}
            className="w-full font-mono"
          />
          <div className="flex gap-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => void checkCloudKey(true)}
              disabled={!apiKey.trim() || health.kind === "checking"}
            >
              {t(
                health.kind === "checking"
                  ? "settings.general.transcriptionProvider.checking"
                  : "settings.general.transcriptionProvider.verifyAndSave",
              )}
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => void checkCloudKey(false)}
              disabled={health.kind === "checking"}
            >
              {t("settings.general.transcriptionProvider.checkSavedKey")}
            </Button>
          </div>
          {health.kind === "verified" && (
            <Alert variant="success">
              {t(
                health.saved
                  ? "settings.general.transcriptionProvider.keySaved"
                  : "settings.general.transcriptionProvider.keyVerified",
              )}
            </Alert>
          )}
          {health.kind === "error" && (
            <Alert variant="error">{health.message}</Alert>
          )}
        </SettingContainer>
      )}
      {provider === "warp_wispr" && (
        <SettingContainer
          title={t(
            "settings.general.transcriptionProvider.credentialsFileTitle",
          )}
          description={t(
            "settings.general.transcriptionProvider.credentialsFileDescription",
          )}
          grouped={true}
          layout="stacked"
        >
          <Input
            type="text"
            value={credentialsFile}
            onChange={(event) => {
              setCredentialsFile(event.target.value);
              setHealth({ kind: "idle" });
            }}
            onBlur={() =>
              updateSetting("warp_credentials_file", credentialsFile)
            }
            placeholder={t(
              "settings.general.transcriptionProvider.credentialsFilePlaceholder",
            )}
            disabled={isUpdating("warp_credentials_file")}
            className="w-full font-mono"
          />
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => void checkConnection()}
            disabled={health.kind === "checking"}
          >
            {health.kind === "checking"
              ? t("settings.general.transcriptionProvider.checking")
              : t("settings.general.transcriptionProvider.checkConnection")}
          </Button>
          {health.kind === "active" && (
            <Alert variant="success">
              {t("settings.general.transcriptionProvider.active", {
                source:
                  health.source === "secret_service"
                    ? t("settings.general.transcriptionProvider.secretService")
                    : t(
                        "settings.general.transcriptionProvider.credentialsFile",
                      ),
                latency: health.latencyMs,
              })}
            </Alert>
          )}
          {health.kind === "error" && (
            <Alert variant="error">{health.message}</Alert>
          )}
        </SettingContainer>
      )}
    </>
  );
};

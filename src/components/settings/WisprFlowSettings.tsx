import { type FC, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Alert } from "@/components/ui/Alert";
import { Button } from "@/components/ui/Button";
import { SettingContainer } from "@/components/ui/SettingContainer";

type Status =
  | "loading"
  | "missing"
  | "saved"
  | "signingIn"
  | "checking"
  | "verified";

export const WisprFlowSettings: FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState<string | null>(null);
  const [hasSession, setHasSession] = useState(false);
  const busy =
    status === "loading" || status === "signingIn" || status === "checking";

  useEffect(() => {
    let active = true;
    void commands
      .wisprFlowSessionStatus()
      .then((saved) => {
        if (!active) return;
        setHasSession(saved);
        setStatus(saved ? "saved" : "missing");
      })
      .catch(() => {
        if (active) setStatus("missing");
      });
    return () => {
      active = false;
    };
  }, []);

  const connect = async (signIn: boolean) => {
    setError(null);
    setStatus(signIn ? "signingIn" : "checking");
    try {
      const result = signIn
        ? await commands.signInWisprFlow()
        : await commands.checkWisprFlowSession();
      if (result.status === "error") {
        setError(result.error);
        setStatus(hasSession ? "saved" : "missing");
      } else {
        setHasSession(true);
        setStatus("verified");
      }
    } catch (cause) {
      setError(String(cause));
      setStatus(hasSession ? "saved" : "missing");
    }
  };

  return (
    <SettingContainer
      title={t("settings.general.transcriptionProvider.wisprSessionTitle")}
      description={t(
        "settings.general.transcriptionProvider.wisprSessionDescription",
      )}
      grouped
      layout="stacked"
    >
      <p className="text-sm text-text/70">
        {t(`settings.general.transcriptionProvider.wisprStatus.${status}`)}
      </p>
      <div className="flex gap-2">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={busy}
          onClick={() => void connect(true)}
        >
          {t("settings.general.transcriptionProvider.wisprSignIn")}
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={busy || !hasSession}
          onClick={() => void connect(false)}
        >
          {t("settings.general.transcriptionProvider.wisprCheck")}
        </Button>
      </div>
      {status === "verified" && (
        <Alert variant="success">
          {t("settings.general.transcriptionProvider.wisprVerified")}
        </Alert>
      )}
      {error && <Alert variant="error">{error}</Alert>}
    </SettingContainer>
  );
};

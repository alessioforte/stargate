import { Button } from "@mantine/core";
import {
  LuRefreshCw,
  LuRotateCcw,
  LuSave,
  LuWandSparkles,
} from "react-icons/lu";
import { useTranslations } from "@/i18n";

interface Props {
  dirty: boolean;
  loading: boolean;
  saveDisabled: boolean;
  validating: boolean;
  onReload: () => void;
  onReset: () => void;
  onSave: () => void;
  onValidate: () => void;
}

const AccessControlToolbar: React.FC<Props> = ({
  dirty,
  loading,
  saveDisabled,
  validating,
  onReload,
  onReset,
  onSave,
  onValidate,
}) => {
  const t = useTranslations();

  return (
    <>
      <Button
        type="button"
        variant="default"
        leftSection={<LuRefreshCw size={15} />}
        loading={loading}
        onClick={onReload}
      >
        {t("reload")}
      </Button>
      <Button
        type="button"
        variant="default"
        leftSection={<LuRotateCcw size={15} />}
        disabled={!dirty}
        onClick={onReset}
      >
        {t("resetChanges")}
      </Button>
      <Button
        type="button"
        variant="light"
        leftSection={<LuWandSparkles size={15} />}
        loading={validating}
        onClick={onValidate}
      >
        {t("validateDraft")}
      </Button>
      <Button
        type="button"
        leftSection={<LuSave size={15} />}
        disabled={saveDisabled}
        loading={loading}
        onClick={onSave}
      >
        {t("saveChanges")}
      </Button>
    </>
  );
};

export default AccessControlToolbar;

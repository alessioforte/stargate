import {
  ActionIcon,
  Box,
  Button,
  Checkbox,
  Group,
  Select,
  Stack,
  TagsInput,
  Text,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { AiOutlineDelete } from "react-icons/ai";
import { FaPlus } from "react-icons/fa6";
import { useTranslations } from "@/i18n";
import { createMatchValues, type MatchValues } from "./match-expression-utils";
import {
  isMatchKind,
  isPathOperator,
  isValueOperator,
  matchKindOptions,
  pathOperatorOptions,
  valueOperatorOptions,
} from "./schema-options";

interface Props {
  value: MatchValues;
  onChange: (value: MatchValues) => void;
}

function ValuePredicateFields({
  value,
  onChange,
}: {
  value: MatchValues;
  onChange: (value: MatchValues) => void;
}) {
  const t = useTranslations();

  return (
    <>
      <Select
        allowDeselect={false}
        variant="filled"
        label={t("operator")}
        data={valueOperatorOptions}
        value={value.valueOperator}
        onChange={(operator) =>
          onChange({
            ...value,
            valueOperator: isValueOperator(operator)
              ? operator
              : value.valueOperator,
          })
        }
      />
      {value.valueOperator === "present" && (
        <Checkbox
          label={t("present")}
          checked={value.present}
          onChange={(event) =>
            onChange({ ...value, present: event.currentTarget.checked })
          }
        />
      )}
      {value.valueOperator === "one_of" && (
        <TagsInput
          variant="filled"
          label={t("matchValues")}
          value={value.valueValues}
          onChange={(valueValues) => onChange({ ...value, valueValues })}
        />
      )}
      {value.valueOperator !== "present" &&
        value.valueOperator !== "one_of" && (
          <TextInput
            variant="filled"
            label={t("matchValue")}
            value={value.valueText}
            onChange={(event) =>
              onChange({ ...value, valueText: event.currentTarget.value })
            }
          />
        )}
    </>
  );
}

const MatchExpressionEditor: React.FC<Props> = ({ value, onChange }) => {
  const t = useTranslations();

  const setChild = (id: string, nextChild: MatchValues) => {
    onChange({
      ...value,
      children: value.children.map((child) =>
        child.id === id ? nextChild : child,
      ),
    });
  };

  const removeChild = (id: string) => {
    onChange({
      ...value,
      children: value.children.filter((child) => child.id !== id),
    });
  };

  return (
    <Stack gap="sm">
      <Select
        allowDeselect={false}
        variant="filled"
        label={t("matchType")}
        data={matchKindOptions}
        value={value.kind}
        onChange={(kind) =>
          onChange({
            ...value,
            kind: isMatchKind(kind) ? kind : value.kind,
            children:
              kind === "all" || kind === "any" || kind === "not"
                ? value.children.length
                  ? value.children
                  : [createMatchValues()]
                : value.children,
          })
        }
      />

      {value.kind === "path" && (
        <Group grow align="flex-start">
          <Select
            allowDeselect={false}
            variant="filled"
            label={t("operator")}
            data={pathOperatorOptions}
            value={value.pathOperator}
            onChange={(operator) =>
              onChange({
                ...value,
                pathOperator: isPathOperator(operator)
                  ? operator
                  : value.pathOperator,
              })
            }
          />
          <TextInput
            variant="filled"
            label={t("matchValue")}
            value={value.pathValue}
            onChange={(event) =>
              onChange({ ...value, pathValue: event.currentTarget.value })
            }
          />
        </Group>
      )}

      {value.kind === "method" && (
        <TagsInput
          variant="filled"
          label={t("methods")}
          value={value.methods}
          onChange={(methods) => onChange({ ...value, methods })}
        />
      )}

      {value.kind === "source_ip" && (
        <TagsInput
          variant="filled"
          label={t("cidrs")}
          value={value.cidrs}
          onChange={(cidrs) => onChange({ ...value, cidrs })}
        />
      )}

      {value.kind === "host" && (
        <ValuePredicateFields value={value} onChange={onChange} />
      )}

      {(value.kind === "header" ||
        value.kind === "query" ||
        value.kind === "cookie") && (
        <>
          <TextInput
            variant="filled"
            label={t("name")}
            value={value.name}
            onChange={(event) =>
              onChange({ ...value, name: event.currentTarget.value })
            }
          />
          <ValuePredicateFields value={value} onChange={onChange} />
        </>
      )}

      {(value.kind === "all" || value.kind === "any") && (
        <Stack gap="xs">
          <Group justify="space-between">
            <Text fw={700}>{t("conditions")}</Text>
            <Button
              type="button"
              size="compact-sm"
              variant="light"
              leftSection={<FaPlus />}
              onClick={() =>
                onChange({
                  ...value,
                  children: [...value.children, createMatchValues()],
                })
              }
            >
              {t("addCondition")}
            </Button>
          </Group>
          {value.children.map((child) => (
            <Box
              key={child.id}
              p="xs"
              style={{
                border: "1px solid var(--mantine-color-default-border)",
                borderRadius: 6,
              }}
            >
              <Group justify="flex-end" mb="xs">
                <Tooltip label={t("delete")}>
                  <ActionIcon
                    type="button"
                    color="red"
                    variant="light"
                    disabled={value.children.length === 1}
                    onClick={() => removeChild(child.id)}
                  >
                    <AiOutlineDelete />
                  </ActionIcon>
                </Tooltip>
              </Group>
              <MatchExpressionEditor
                value={child}
                onChange={(nextChild) => setChild(child.id, nextChild)}
              />
            </Box>
          ))}
        </Stack>
      )}

      {value.kind === "not" && (
        <Box
          p="xs"
          style={{
            border: "1px solid var(--mantine-color-default-border)",
            borderRadius: 6,
          }}
        >
          <Text fw={700} mb="xs">
            {t("negatedCondition")}
          </Text>
          <MatchExpressionEditor
            value={value.children[0] ?? createMatchValues()}
            onChange={(child) => onChange({ ...value, children: [child] })}
          />
        </Box>
      )}
    </Stack>
  );
};

export default MatchExpressionEditor;

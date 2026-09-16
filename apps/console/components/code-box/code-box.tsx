import React from "react";
import { useComputedColorScheme } from "@mantine/core";
import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";
import { monokaiDimmed } from "@uiw/codemirror-theme-monokai-dimmed";

interface Props {
  value: string;
  readOnly?: boolean;
  onChange: (value: string) => void;
  language?: "json" | "yaml";
}

const CodeBox: React.FC<Props> = ({
  value,
  readOnly = false,
  onChange,
  language = "json",
}) => {
  const colorScheme = useComputedColorScheme("light");
  return (
    <CodeMirror
      readOnly={readOnly}
      indentWithTab
      style={{ height: "100%" }}
      value={value}
      theme={colorScheme === "dark" ? monokaiDimmed : colorScheme}
      onError={console.log}
      extensions={[language === "json" ? json() : yaml()]}
      onChange={onChange}
    />
  );
};

export default CodeBox;

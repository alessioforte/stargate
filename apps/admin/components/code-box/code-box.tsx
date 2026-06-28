import React from "react";
import { useMantineColorScheme } from "@mantine/core";
import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { monokaiDimmed } from "@uiw/codemirror-theme-monokai-dimmed";

interface Props {
  value: string;
  readOnly?: boolean;
  onChange: (value: string) => void;
}

const CodeBox: React.FC<Props> = ({ value, readOnly = false, onChange }) => {
  const theme = useMantineColorScheme();
  return (
    <CodeMirror
      readOnly={readOnly}
      indentWithTab
      style={{ height: "100%" }}
      value={value}
      theme={
        theme.colorScheme === "dark"
          ? monokaiDimmed
          : (theme.colorScheme as "dark" | "light")
      }
      onError={console.log}
      extensions={[json()]}
      onChange={onChange}
    />
  );
};

export default CodeBox;

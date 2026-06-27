import React from "react";
import { ScrollArea, useMantineColorScheme } from "@mantine/core";
import { useResizeObserver } from "@mantine/hooks";
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
  const [ref, rect] = useResizeObserver();
  return (
    <ScrollArea ref={ref} h="100%">
      <CodeMirror
        readOnly={readOnly}
        indentWithTab
        height={`${rect?.height}px`}
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
    </ScrollArea>
  );
};

export default CodeBox;

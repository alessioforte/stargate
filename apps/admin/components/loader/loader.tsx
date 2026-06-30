import SparkleSpinner from "../sparkle-loader/sparkle-loader";
import { Center } from "@mantine/core";

export default function Loader() {
  return (
    <Center h="100%">
      <SparkleSpinner size={120} />
    </Center>
  );
}

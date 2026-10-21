import {
  Body,
  Button,
  Column,
  Container,
  Head,
  Heading,
  Html,
  Preview,
  Row,
  Section,
  Tailwind,
  Text,
} from "@react-email/components";

interface SignupAccountExistsProps {
  loginUrl: string;
}

export const SignupAccountExists = ({
  loginUrl = "{{link}}",
}: SignupAccountExistsProps) => {
  return (
    <Html>
      <Tailwind>
        <Head />
        <Body style={main}>
          <Preview>You already have an account</Preview>
          <Container className="pt-10">
            <Section>
              <Row className="p-5">
                <Column>
                  <Heading className="text-[32px] font-bold text-center">
                    You already have an account
                  </Heading>

                  <Text className="text-base">
                    We received a request to create an account with this email
                    address, but an account already uses it.
                  </Text>
                  <Text className="text-base">
                    You can sign in with the button below. If you don't
                    remember your password, choose "Forgot password" on the
                    sign-in page.
                  </Text>

                  <Row className="my-20">
                    <Column className="text-center" colSpan={2}>
                      <Button
                        className="bg-[#007ee6] rounded border border-solid border-black/10 text-white font-bold cursor-pointer inline-block px-[30px] py-3 no-underline"
                        href={loginUrl}
                      >
                        Sign In
                      </Button>
                    </Column>
                  </Row>

                  <Text className="text-base -mt-[5px]">
                    If you didn't request this, you can ignore this message:
                    your account has not changed.
                  </Text>
                </Column>
              </Row>
            </Section>
            <Text className="text-center text-xs leading-[24px] text-black/70">
              © {new Date().getFullYear()} | Stargate Inc. All rights reserved.
            </Text>
          </Container>
        </Body>
      </Tailwind>
    </Html>
  );
};

export default SignupAccountExists;

const main = {
  fontFamily:
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen', 'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif",
};

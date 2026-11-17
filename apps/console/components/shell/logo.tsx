interface Props {
  size?: number;
}

const Logo: React.FC<Props> = ({ size = 28 }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 64 64"
    width={size}
    height={size}
    aria-hidden="true"
  >
    <rect width="64" height="64" rx="15" fill="#14171c" />
    <path
      d="M32 9l5.9 16.1L54 31.7l-16.1 6.4L32 55l-5.9-16.9L10 31.7l16.1-6.6L32 9z"
      fill="var(--app-brand-mark, #e3a008)"
    />
    <circle cx="32" cy="32" r="5.5" fill="#14171c" />
  </svg>
);

export default Logo;

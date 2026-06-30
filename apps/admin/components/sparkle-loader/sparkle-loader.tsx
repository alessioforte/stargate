import { useId } from "react";

function SparkleSpinner({
  size = 64,
  speed = 1,
  label = "Loading",
  className = "",
  ...rest
}) {
  const uid = useId().replace(/:/g, "");
  const gradId = `spark-${uid}`;
  const glowId = `glow-${uid}`;
  const sparkId = `path-${uid}`;
  const pulse = `${(1.6 / speed).toFixed(3)}s`;
  const rot = `${(6 / speed).toFixed(3)}s`;

  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 100 100"
      width={size}
      height={size}
      role="img"
      aria-label={label}
      className={className}
      {...rest}
    >
      <title>{label}</title>
      <defs>
        <linearGradient id={gradId} x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#FEF08A" />
          <stop offset="50%" stopColor="#FACC15" />
          <stop offset="100%" stopColor="#CA8A04" />
        </linearGradient>
        <radialGradient id={glowId} cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#FACC15" stopOpacity="0.5" />
          <stop offset="100%" stopColor="#FACC15" stopOpacity="0" />
        </radialGradient>
        <path
          id={sparkId}
          d="M0,-22 C2,-6 6,-2 22,0 C6,2 2,6 0,22 C-2,6 -6,2 -22,0 C-6,-2 -2,-6 0,-22 Z"
        />
      </defs>
      <g>
        <animateTransform
          attributeName="transform"
          type="rotate"
          values="0 50 50;360 50 50"
          dur={rot}
          repeatCount="indefinite"
        />
        <circle cx="50" cy="50" r="30" fill={`url(#${glowId})`}>
          <animate
            attributeName="r"
            values="22;32;22"
            dur={pulse}
            repeatCount="indefinite"
            calcMode="spline"
            keyTimes="0;0.5;1"
            keySplines="0.4 0 0.6 1;0.4 0 0.6 1"
          />
        </circle>
        <g transform="translate(50,50)">
          <g>
            <use href={`#${sparkId}`} fill={`url(#${gradId})`} />
            <animateTransform
              attributeName="transform"
              type="scale"
              values="0.85;1.15;0.85"
              dur={pulse}
              repeatCount="indefinite"
              calcMode="spline"
              keyTimes="0;0.5;1"
              keySplines="0.4 0 0.6 1;0.4 0 0.6 1"
            />
          </g>
        </g>
        <g transform="translate(80,26)">
          <g transform="scale(0.42)">
            <use href={`#${sparkId}`} fill="#FDE047" />
            <animateTransform
              attributeName="transform"
              type="scale"
              values="0;0.5;0"
              dur={pulse}
              begin="0s"
              repeatCount="indefinite"
            />
            <animate
              attributeName="opacity"
              values="0;1;0"
              dur={pulse}
              begin="0s"
              repeatCount="indefinite"
            />
          </g>
        </g>
        <g transform="translate(78,74)">
          <g transform="scale(0.34)">
            <use href={`#${sparkId}`} fill="#EAB308" />
            <animateTransform
              attributeName="transform"
              type="scale"
              values="0;0.5;0"
              dur={pulse}
              begin={`${(0.55 / speed).toFixed(3)}s`}
              repeatCount="indefinite"
            />
            <animate
              attributeName="opacity"
              values="0;1;0"
              dur={pulse}
              begin={`${(0.55 / speed).toFixed(3)}s`}
              repeatCount="indefinite"
            />
          </g>
        </g>
        <g transform="translate(22,66)">
          <g transform="scale(0.38)">
            <use href={`#${sparkId}`} fill="#FACC15" />
            <animateTransform
              attributeName="transform"
              type="scale"
              values="0;0.5;0"
              dur={pulse}
              begin={`${(1.1 / speed).toFixed(3)}s`}
              repeatCount="indefinite"
            />
            <animate
              attributeName="opacity"
              values="0;1;0"
              dur={pulse}
              begin={`${(1.1 / speed).toFixed(3)}s`}
              repeatCount="indefinite"
            />
          </g>
        </g>
        <circle cx="26" cy="30" r="1.6" fill="#FEF08A">
          <animate
            attributeName="opacity"
            values="0;1;0"
            dur={pulse}
            begin={`${(0.3 / speed).toFixed(3)}s`}
            repeatCount="indefinite"
          />
        </circle>
        <circle cx="70" cy="50" r="1.4" fill="#FDE047">
          <animate
            attributeName="opacity"
            values="0;1;0"
            dur={pulse}
            begin={`${(0.9 / speed).toFixed(3)}s`}
            repeatCount="indefinite"
          />
        </circle>
      </g>
    </svg>
  );
}

export default SparkleSpinner;

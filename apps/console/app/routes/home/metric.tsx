import classes from "./home.module.css";

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className={classes.metric}>
      <div className={classes.statHead}>{label}</div>
      <div className={classes.statValue}>{value.toLocaleString()}</div>
    </div>
  );
}

export default Metric;

/**
 * Status Usage:
 * - IDLE: Default state, before any operation begins or after an operation concludes/reset.
 * - LOADING: When an asynchronous operation (like data fetching) is in progress.
 * - ERROR: When an operation fails or encounters an issue, to handle and display errors.
 * - SUCCESS: When an operation completes successfully.
 */
export type Status = "IDLE" | "LOADING" | "ERROR" | "SUCCESS";

/**
 * StoreItem is a class that represents a generic store item.
 * It is used to manage the state of a single item in the store.
 * It provides methods to set and update the state of the item.
 * @template Data The type of data that the item holds.
 * @template MetaData The type of metadata associated with the data.
 * @param data The data associated with the item.
 * @param meta The metadata associated with the data.
 * an error message in case of an error status).
 */
export class StoreItem<Data = undefined, MetaData = undefined> {
  public data?: Data | null;
  public meta?: MetaData | null;
  public status: Status = "IDLE";
  public message?: string;

  constructor(data?: Data | null, meta?: MetaData | null) {
    this.data = data;
    this.meta = meta;
  }

  public reset() {
    this.data = null;
    this.meta = null;
    this.status = "IDLE";
    this.message = undefined;
    return this;
  }

  public setLoading() {
    this.status = "LOADING";
    return this;
  }

  public setError(message?: string) {
    this.status = "ERROR";
    this.message = message;
    return this;
  }

  public setSuccess(data: Data | null, meta?: MetaData) {
    this.data = data;
    this.meta = meta;
    this.status = "SUCCESS";
    return this;
  }

  public setData(data: Data | null) {
    this.data = data;
    return this;
  }

  public setMeta(meta: MetaData) {
    this.meta = meta;
    return this;
  }

  public setMessage(message: string) {
    this.message = message;
    return this;
  }

  public getData() {
    return this.data;
  }

  public isLoading() {
    return this.status === "LOADING";
  }

  public isError() {
    return this.status === "ERROR";
  }

  public isSuccess() {
    return this.status === "SUCCESS";
  }

  public isIdle() {
    return this.status === "IDLE";
  }
}

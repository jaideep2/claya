import { useEffect, useState } from "react";
import { host } from "@host";
import {
  Badge, Box, Button, Card, Dialog, Flex, Grid, Heading, IconButton,
  Progress, SegmentedControl, Switch, Text, TextArea, TextField,
} from "@ui";

/**
 * A day board: small widgets over one shared store.
 *
 * Deliberately not a charts-and-metrics dashboard — with no external data source
 * that is a screenful of zeroes. This one is useful on its own and shows the shape
 * a richer board would take.
 */
type Kind = "counter" | "check" | "note" | "goal";

interface Tile {
  id: number;
  kind: Kind;
  label: string;
  count: number;
  target: number;
  done: boolean;
  text: string;
}

export const schema = {
  tiles: {
    type: "record-list",
    key: "id",
    fields: ["id", "kind", "label", "count", "target", "done", "text"],
  },
};

const KINDS: Kind[] = ["counter", "check", "note", "goal"];

export default function App() {
  const [tiles, setTiles] = useState<Tile[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<Kind>("counter");
  const [label, setLabel] = useState("");
  const [target, setTarget] = useState("5");

  useEffect(() => {
    host.state.get<Tile[]>("tiles", []).then((t) => {
      setTiles(t);
      setLoaded(true);
    });
  }, []);

  useEffect(() => {
    if (loaded) host.state.set("tiles", tiles);
  }, [tiles, loaded]);

  const patch = (id: number, next: Partial<Tile>) =>
    setTiles((t) => t.map((x) => (x.id === id ? { ...x, ...next } : x)));

  const add = () => {
    const name = label.trim();
    if (!name) return;
    setTiles((t) => [
      ...t,
      {
        id: Date.now(),
        kind,
        label: name,
        count: 0,
        target: Math.max(1, Number(target) || 5),
        done: false,
        text: "",
      },
    ]);
    setLabel("");
    setOpen(false);
  };

  const doneCount = tiles.filter((t) => t.kind === "check" && t.done).length;
  const checkCount = tiles.filter((t) => t.kind === "check").length;

  return (
    <Box p="5">
      <Flex align="baseline" justify="between" mb="4">
        <Heading size="6">Board</Heading>
        <Flex align="center" gap="3">
          {checkCount > 0 && (
            <Badge variant="soft" color={doneCount === checkCount ? "green" : "gray"}>
              {doneCount}/{checkCount} done
            </Badge>
          )}
          <Dialog.Root open={open} onOpenChange={setOpen}>
            <Dialog.Trigger>
              <Button size="2">Add tile</Button>
            </Dialog.Trigger>
            <Dialog.Content maxWidth="420px">
              <Dialog.Title>New tile</Dialog.Title>
              <Flex direction="column" gap="3" mt="3">
                <SegmentedControl.Root
                  value={kind}
                  onValueChange={(v) => setKind(v as Kind)}
                >
                  {KINDS.map((k) => (
                    <SegmentedControl.Item key={k} value={k}>
                      {k}
                    </SegmentedControl.Item>
                  ))}
                </SegmentedControl.Root>
                <TextField.Root
                  value={label}
                  placeholder="Tile name"
                  onChange={(e) => setLabel(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && add()}
                />
                {kind === "goal" && (
                  <TextField.Root
                    type="number"
                    value={target}
                    placeholder="Target"
                    onChange={(e) => setTarget(e.target.value)}
                  />
                )}
                <Flex gap="3" justify="end" mt="1">
                  <Dialog.Close>
                    <Button variant="soft" color="gray">
                      Cancel
                    </Button>
                  </Dialog.Close>
                  <Button onClick={add}>Add</Button>
                </Flex>
              </Flex>
            </Dialog.Content>
          </Dialog.Root>
        </Flex>
      </Flex>

      <Grid columns={{ initial: "1", sm: "2", md: "3" }} gap="3">
        {tiles.map((t) => (
          <Card key={t.id} size="2">
            <Flex direction="column" gap="3" style={{ minHeight: 96 }}>
              <Flex align="start" justify="between" gap="2">
                <Text size="2" weight="bold">
                  {t.label}
                </Text>
                <IconButton
                  size="1"
                  variant="ghost"
                  color="gray"
                  onClick={() => setTiles((all) => all.filter((x) => x.id !== t.id))}
                >
                  ×
                </IconButton>
              </Flex>

              {t.kind === "counter" && (
                <Flex align="center" justify="center" gap="4" mt="auto">
                  <IconButton
                    variant="soft"
                    onClick={() => patch(t.id, { count: Math.max(0, t.count - 1) })}
                  >
                    −
                  </IconButton>
                  <Text size="7" weight="medium" style={{ fontVariantNumeric: "tabular-nums" }}>
                    {t.count}
                  </Text>
                  <IconButton variant="soft" onClick={() => patch(t.id, { count: t.count + 1 })}>
                    +
                  </IconButton>
                </Flex>
              )}

              {t.kind === "check" && (
                <Flex align="center" gap="2" mt="auto">
                  <Switch checked={t.done} onCheckedChange={(v) => patch(t.id, { done: v })} />
                  <Text size="2" color="gray">
                    {t.done ? "done" : "not yet"}
                  </Text>
                </Flex>
              )}

              {t.kind === "note" && (
                <TextArea
                  size="1"
                  value={t.text}
                  placeholder="…"
                  onChange={(e) => patch(t.id, { text: e.target.value })}
                  style={{ minHeight: 64 }}
                />
              )}

              {t.kind === "goal" && (
                <Flex direction="column" gap="2" mt="auto">
                  <Flex align="center" justify="between">
                    <Text size="1" color="gray">
                      {t.count} / {t.target}
                    </Text>
                    <IconButton
                      size="1"
                      variant="soft"
                      onClick={() => patch(t.id, { count: t.count + 1 })}
                    >
                      +
                    </IconButton>
                  </Flex>
                  <Progress value={Math.min(100, (t.count / t.target) * 100)} />
                </Flex>
              )}
            </Flex>
          </Card>
        ))}
      </Grid>

      {tiles.length === 0 && (
        <Box py="8">
          <Text size="2" color="gray" align="center" as="div">
            Add a tile to start.
          </Text>
        </Box>
      )}
    </Box>
  );
}

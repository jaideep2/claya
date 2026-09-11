import { useEffect, useState } from "react";
import { host } from "@host";
import {
  Badge, Box, Button, Card, Flex, Heading, IconButton,
  Progress, Table, Text, TextField,
} from "@ui";

interface Habit {
  id: number;
  name: string;
  days: string[];
}

export const schema = {
  habits: { type: "record-list", key: "id", fields: ["id", "name", "days"] },
};

const iso = (d: Date) => d.toISOString().slice(0, 10);
const week = () =>
  Array.from({ length: 7 }, (_, i) => {
    const d = new Date();
    d.setDate(d.getDate() - (6 - i));
    return d;
  });

export default function App() {
  const [habits, setHabits] = useState<Habit[]>([]);
  const [draft, setDraft] = useState("");
  const [loaded, setLoaded] = useState(false);
  const days = week();

  useEffect(() => {
    host.state.get<Habit[]>("habits", []).then((h) => {
      setHabits(h);
      setLoaded(true);
    });
  }, []);

  useEffect(() => {
    if (loaded) host.state.set("habits", habits);
  }, [habits, loaded]);

  const add = () => {
    const name = draft.trim();
    if (!name) return;
    setHabits((h) => [...h, { id: Date.now(), name, days: [] }]);
    setDraft("");
  };

  const toggle = (id: number, day: string) =>
    setHabits((h) =>
      h.map((x) =>
        x.id === id
          ? {
              ...x,
              days: x.days.includes(day)
                ? x.days.filter((d) => d !== day)
                : [...x.days, day],
            }
          : x
      )
    );

  const scored = habits.map((h) => ({
    ...h,
    hits: h.days.filter((d) => days.some((w) => iso(w) === d)).length,
  }));
  const overall = scored.length
    ? Math.round((scored.reduce((n, h) => n + h.hits, 0) / (scored.length * 7)) * 100)
    : 0;

  return (
    <Box p="5" style={{ maxWidth: 640, margin: "0 auto" }}>
      <Flex align="baseline" justify="between" mb="3">
        <Heading size="6">Tracker</Heading>
        <Text size="2" color="gray">
          last 7 days
        </Text>
      </Flex>

      <Card size="2" mb="4">
        <Flex align="center" gap="3">
          <Text size="2" color="gray" style={{ minWidth: 62 }}>
            {overall}% done
          </Text>
          <Progress value={overall} style={{ flex: 1 }} />
        </Flex>
      </Card>

      <Flex gap="2" mb="4">
        <TextField.Root
          style={{ flex: 1 }}
          value={draft}
          placeholder="Add a habit"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <Button onClick={add}>Add</Button>
      </Flex>

      <Table.Root variant="surface" size="1">
        <Table.Header>
          <Table.Row>
            <Table.ColumnHeaderCell>Habit</Table.ColumnHeaderCell>
            {days.map((d) => (
              <Table.ColumnHeaderCell key={iso(d)} justify="center">
                {d.toLocaleDateString(undefined, { weekday: "narrow" })}
              </Table.ColumnHeaderCell>
            ))}
            <Table.ColumnHeaderCell justify="end">Week</Table.ColumnHeaderCell>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {scored.map((h) => (
            <Table.Row key={h.id}>
              <Table.RowHeaderCell>
                <Flex align="center" gap="2">
                  <Text size="2" truncate>
                    {h.name}
                  </Text>
                  <IconButton
                    size="1"
                    variant="ghost"
                    color="gray"
                    onClick={() => setHabits((all) => all.filter((x) => x.id !== h.id))}
                  >
                    ×
                  </IconButton>
                </Flex>
              </Table.RowHeaderCell>
              {days.map((d) => {
                const day = iso(d);
                const on = h.days.includes(day);
                return (
                  <Table.Cell key={day} justify="center">
                    <IconButton
                      size="1"
                      radius="full"
                      variant={on ? "solid" : "soft"}
                      color={on ? undefined : "gray"}
                      title={day}
                      onClick={() => toggle(h.id, day)}
                    >
                      {on ? "✓" : " "}
                    </IconButton>
                  </Table.Cell>
                );
              })}
              <Table.Cell justify="end">
                <Badge variant="soft" color={h.hits >= 5 ? "green" : "gray"}>
                  {h.hits}/7
                </Badge>
              </Table.Cell>
            </Table.Row>
          ))}
        </Table.Body>
      </Table.Root>

      {habits.length === 0 && (
        <Box py="6">
          <Text size="2" color="gray" align="center" as="div">
            Nothing tracked yet.
          </Text>
        </Box>
      )}
    </Box>
  );
}

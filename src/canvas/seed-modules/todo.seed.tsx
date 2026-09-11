import { useEffect, useMemo, useState } from "react";
import { host } from "@host";
import {
  Badge, Box, Button, Card, Checkbox, Flex, Heading, IconButton,
  ScrollArea, Select, Separator, Tabs, Text, TextField,
} from "@ui";

type Priority = "low" | "normal" | "high";

interface Todo {
  id: number;
  title: string;
  done: boolean;
  priority: Priority;
}

export const schema = {
  todos: { type: "record-list", key: "id", fields: ["id", "title", "done", "priority"] },
};

const TONE: Record<Priority, "gray" | "blue" | "crimson"> = {
  low: "gray",
  normal: "blue",
  high: "crimson",
};

export default function App() {
  const [todos, setTodos] = useState<Todo[]>([]);
  const [draft, setDraft] = useState("");
  const [priority, setPriority] = useState<Priority>("normal");
  const [tab, setTab] = useState("all");
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    host.state.get<Todo[]>("todos", []).then((t) => {
      setTodos(t);
      setLoaded(true);
    });
  }, []);

  useEffect(() => {
    if (loaded) host.state.set("todos", todos);
  }, [todos, loaded]);

  const add = () => {
    const title = draft.trim();
    if (!title) return;
    setTodos((t) => [...t, { id: Date.now(), title, done: false, priority }]);
    setDraft("");
  };

  const shown = useMemo(
    () =>
      todos.filter((t) =>
        tab === "active" ? !t.done : tab === "done" ? t.done : true
      ),
    [todos, tab]
  );

  const remaining = todos.filter((t) => !t.done).length;

  return (
    <Box p="5" style={{ maxWidth: 560, margin: "0 auto" }}>
      <Flex align="baseline" justify="between" mb="4">
        <Heading size="6">Todo</Heading>
        <Text size="2" color="gray">
          {remaining} left
        </Text>
      </Flex>

      <Flex gap="2" mb="4">
        <TextField.Root
          style={{ flex: 1 }}
          value={draft}
          placeholder="What needs doing?"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <Select.Root value={priority} onValueChange={(v) => setPriority(v as Priority)}>
          <Select.Trigger />
          <Select.Content>
            <Select.Item value="low">Low</Select.Item>
            <Select.Item value="normal">Normal</Select.Item>
            <Select.Item value="high">High</Select.Item>
          </Select.Content>
        </Select.Root>
        <Button onClick={add}>Add</Button>
      </Flex>

      <Tabs.Root value={tab} onValueChange={setTab}>
        <Tabs.List>
          <Tabs.Trigger value="all">All</Tabs.Trigger>
          <Tabs.Trigger value="active">Active</Tabs.Trigger>
          <Tabs.Trigger value="done">Done</Tabs.Trigger>
        </Tabs.List>
      </Tabs.Root>

      <ScrollArea style={{ maxHeight: 420 }} mt="3">
        <Flex direction="column" gap="2">
          {shown.map((t) => (
            <Card key={t.id} size="1">
              <Flex align="center" gap="3">
                <Checkbox
                  checked={t.done}
                  onCheckedChange={() =>
                    setTodos((all) =>
                      all.map((x) => (x.id === t.id ? { ...x, done: !x.done } : x))
                    )
                  }
                />
                <Text
                  size="2"
                  style={{
                    flex: 1,
                    textDecoration: t.done ? "line-through" : undefined,
                    opacity: t.done ? 0.5 : 1,
                  }}
                >
                  {t.title}
                </Text>
                <Badge color={TONE[t.priority]} variant="soft">
                  {t.priority}
                </Badge>
                <IconButton
                  size="1"
                  variant="ghost"
                  color="gray"
                  onClick={() => setTodos((all) => all.filter((x) => x.id !== t.id))}
                >
                  ×
                </IconButton>
              </Flex>
            </Card>
          ))}
          {shown.length === 0 && (
            <Box py="6">
              <Text size="2" color="gray" align="center" as="div">
                Nothing here.
              </Text>
            </Box>
          )}
        </Flex>
      </ScrollArea>

      {todos.some((t) => t.done) && (
        <>
          <Separator my="3" size="4" />
          <Button
            variant="soft"
            color="gray"
            size="1"
            onClick={() => setTodos((all) => all.filter((t) => !t.done))}
          >
            Clear completed
          </Button>
        </>
      )}
    </Box>
  );
}

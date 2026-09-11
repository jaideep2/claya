import { useEffect, useState } from "react";
import { host } from "@host";
import {
  AlertDialog, Badge, Box, Button, Card, Flex, Grid, Heading,
  ScrollArea, Separator, Text, TextArea, TextField,
} from "@ui";

interface Note {
  id: number;
  title: string;
  body: string;
  updated: number;
}

export const schema = {
  notes: { type: "record-list", key: "id", fields: ["id", "title", "body", "updated"] },
};

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [openId, setOpenId] = useState<number | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    host.state.get<Note[]>("notes", []).then((n) => {
      setNotes(n);
      setOpenId(n[0]?.id ?? null);
      setLoaded(true);
    });
  }, []);

  useEffect(() => {
    if (loaded) host.state.set("notes", notes);
  }, [notes, loaded]);

  const open = notes.find((n) => n.id === openId) ?? null;

  const add = () => {
    const note: Note = { id: Date.now(), title: "Untitled", body: "", updated: Date.now() };
    setNotes((n) => [note, ...n]);
    setOpenId(note.id);
  };

  const edit = (patch: Partial<Note>) =>
    setNotes((n) =>
      n.map((x) => (x.id === openId ? { ...x, ...patch, updated: Date.now() } : x))
    );

  const remove = (id: number) => {
    setNotes((n) => n.filter((x) => x.id !== id));
    setOpenId(null);
  };

  return (
    <Box p="5">
      <Grid columns="210px 1fr" gap="4">
        <Flex direction="column" gap="2">
          <Button onClick={add}>New note</Button>
          <ScrollArea style={{ maxHeight: 520 }}>
            <Flex direction="column" gap="1" pr="2">
              {notes.map((n) => (
                <Card
                  key={n.id}
                  size="1"
                  variant={n.id === openId ? "surface" : "ghost"}
                  onClick={() => setOpenId(n.id)}
                  style={{ cursor: "pointer" }}
                >
                  <Text as="div" size="2" weight={n.id === openId ? "bold" : "regular"} truncate>
                    {n.title || "Untitled"}
                  </Text>
                  <Text as="div" size="1" color="gray">
                    {new Date(n.updated).toLocaleDateString()}
                  </Text>
                </Card>
              ))}
              {notes.length === 0 && (
                <Text size="2" color="gray" align="center" as="div" mt="4">
                  No notes yet.
                </Text>
              )}
            </Flex>
          </ScrollArea>
        </Flex>

        {open ? (
          <Flex direction="column" gap="3">
            <TextField.Root
              size="3"
              value={open.title}
              placeholder="Title"
              onChange={(e) => edit({ title: e.target.value })}
            />
            <TextArea
              size="3"
              style={{ minHeight: 380 }}
              value={open.body}
              placeholder="Write something…"
              onChange={(e) => edit({ body: e.target.value })}
            />
            <Separator size="4" />
            <Flex align="center" justify="between">
              <Badge color="gray" variant="soft">
                {open.body.trim() ? open.body.trim().split(/\s+/).length : 0} words
              </Badge>
              <AlertDialog.Root>
                <AlertDialog.Trigger>
                  <Button variant="soft" color="red" size="1">
                    Delete
                  </Button>
                </AlertDialog.Trigger>
                <AlertDialog.Content maxWidth="420px">
                  <AlertDialog.Title>Delete this note?</AlertDialog.Title>
                  <AlertDialog.Description size="2">
                    “{open.title || "Untitled"}” will be removed. This cannot be undone
                    from here.
                  </AlertDialog.Description>
                  <Flex gap="3" mt="4" justify="end">
                    <AlertDialog.Cancel>
                      <Button variant="soft" color="gray">
                        Cancel
                      </Button>
                    </AlertDialog.Cancel>
                    <AlertDialog.Action>
                      <Button color="red" onClick={() => remove(open.id)}>
                        Delete
                      </Button>
                    </AlertDialog.Action>
                  </Flex>
                </AlertDialog.Content>
              </AlertDialog.Root>
            </Flex>
          </Flex>
        ) : (
          <Flex align="center" justify="center" style={{ minHeight: 380 }}>
            <Text size="2" color="gray">
              Select a note, or make a new one.
            </Text>
          </Flex>
        )}
      </Grid>
    </Box>
  );
}
